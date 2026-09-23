// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Differential MkDocs RSS and JSON Feed generation.

use anyhow::Result;
use regex::Regex;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Stream, StreamSetExt, StreamTupleExt, Value};

use crate::config::plugins::RssPluginConfig;
use crate::config::{Config, Project};
use crate::path::SourceRoot;
use crate::structure::dynamic::Dynamic;
use crate::structure::page::{Page, PageDescriptor, PageOrigin};
use crate::workflow::{output::Artifact, Configuration};

mod date;
mod feed;

use date::{git_dates, resolve, Stamp};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// RSS plugin and its compiled instance filters.
#[derive(Clone, Debug)]
pub struct Rss {
    /// Enabled instances, shared across stream closures.
    instances: Arc<Vec<Instance>>,
    /// Site metadata used by feed serialization.
    project: Arc<Project>,
    /// Source root used to locate local item images.
    docs: SourceRoot,
}

/// One enabled instance and its compiled page filter.
#[derive(Clone, Debug)]
struct Instance {
    /// Output and page selection settings.
    config: RssPluginConfig,
    /// Compilation failure retained for stream error propagation.
    path: Result<Regex, String>,
}

/// One page's feed contribution for one instance.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// Final page and its metadata.
    page: Page,
    /// Original Markdown body used for excerpts.
    body: Arc<str>,
    /// Creation date, or the current feed build time.
    created: Option<Stamp>,
    /// Update date, or the current feed build time.
    updated: Option<Stamp>,
}

/// A seed keeps an empty feed alive when no pages match.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    /// Owning RSS instance.
    instance: usize,
    /// Page entry, or `None` for the empty-feed seed.
    entry: Option<Entry>,
}

/// The only global ordering boundary: two bounded top-k selections.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ranked {
    /// Owning RSS instance.
    instance: usize,
    /// Timestamp captured once for this instance's changed revision.
    built: Stamp,
    /// Bounded entries ordered by creation date.
    created: Vec<Entry>,
    /// Bounded entries ordered by update date.
    updated: Vec<Entry>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Rss {
    /// Prepares enabled instances and their path filters.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        let instances = config
            .project
            .plugins
            .rss
            .config
            .iter()
            .filter(|instance| {
                instance.config.enabled
                    && (instance.config.rss_feed_enabled
                        || instance.config.json_feed_enabled)
            })
            .map(|instance| Instance {
                path: Regex::new(&instance.config.match_path).map_err(
                    |error| format!("invalid rss match_path: {error}"),
                ),
                config: instance.config.clone(),
            })
            .collect();
        Self {
            instances: Arc::new(instances),
            project: config.project.clone(),
            docs: config.docs_root().clone(),
        }
    }

    /// Produces feed artifacts from final pages and their original Markdown.
    // Keep the seed, join, bounded reduction, and output stages together.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn setup(
        &self, pages: &Stream<Id, Page>,
        descriptors: &Stream<Id, PageDescriptor>,
        configuration: &Stream<Id, Configuration>,
    ) -> Stream<Id, Artifact> {
        if self.instances.is_empty() {
            return configuration.flat_map(|_: &Configuration| {
                Ok::<Vec<(Key<Id>, Artifact)>, anyhow::Error>(Vec::new())
            });
        }
        let instances = self.instances.clone();
        let seed = configuration.flat_map(move |_config: &Configuration| {
            instances
                .iter()
                .enumerate()
                .map(|(index, instance)| {
                    instance
                        .path
                        .as_ref()
                        .map_err(|error| anyhow::anyhow!("{error}"))?;
                    Ok((
                        candidate_key("rss-seed", index, "seed"),
                        Candidate { instance: index, entry: None },
                    ))
                })
                .collect::<Result<Vec<_>>>()
        });

        let rss = self.clone();
        let candidates = (pages.clone(), descriptors.clone()).join().flat_map(
            move |(page, descriptor): &(Page, PageDescriptor)| {
                if matches!(page.meta.get("draft"), Some(Dynamic::Bool(true))) {
                    return Ok::<_, anyhow::Error>(Vec::new());
                }
                let eligible = rss
                    .instances
                    .iter()
                    .enumerate()
                    .filter(|(_, instance)| {
                        instance
                            .path
                            .as_ref()
                            .ok()
                            .and_then(|path| path.find(page.source().as_str()))
                            .is_some_and(|found| found.start() == 0)
                    })
                    .collect::<Vec<_>>();
                if eligible.is_empty() {
                    return Ok(Vec::new());
                }
                let git = if eligible
                    .iter()
                    .any(|(_, instance)| instance.config.use_git)
                    && matches!(page.origin(), PageOrigin::Source(_))
                {
                    git_dates(
                        &rss.project.root_dir,
                        &rss.docs.join(page.source()),
                    )
                } else {
                    None
                };
                let body: Arc<str> =
                    Arc::from(descriptor.document.body.as_str());
                eligible
                    .into_iter()
                    .map(|(index, instance)| {
                        let settings = &instance.config.date_from_meta;
                        let created = resolve(
                            &page.meta,
                            &settings.as_creation,
                            settings,
                            instance
                                .config
                                .use_git
                                .then_some(git)
                                .flatten()
                                .map(|dates| dates.0),
                        )?;
                        let updated = resolve(
                            &page.meta,
                            &settings.as_update,
                            settings,
                            instance
                                .config
                                .use_git
                                .then_some(git)
                                .flatten()
                                .map(|dates| dates.1),
                        )?;
                        Ok((
                            candidate_key(
                                "rss-candidate",
                                index,
                                page.source().as_str(),
                            ),
                            Candidate {
                                instance: index,
                                entry: Some(Entry {
                                    page: page.clone(),
                                    body: body.clone(),
                                    created,
                                    updated,
                                }),
                            },
                        ))
                    })
                    .collect::<Result<Vec<_>>>()
            },
        );

        let limits = self.instances.clone();
        let ranked = (seed, candidates).coalesce().reduce_by_key(
            |candidate: &Candidate| instance_key(candidate.instance),
            move |candidates: &dyn Collection<Key<Id>, Candidate>| {
                let instance =
                    candidates.values().next().map(|item| item.instance)?;
                let length = limits[instance].config.length;
                let built = Stamp::now();
                Some(Ranked {
                    instance,
                    built: built.clone(),
                    created: top(
                        candidates
                            .values()
                            .filter_map(|item| item.entry.as_ref()),
                        length,
                        |entry| {
                            entry
                                .created
                                .as_ref()
                                .map_or(built.epoch, |date| date.epoch)
                        },
                    ),
                    updated: top(
                        candidates
                            .values()
                            .filter_map(|item| item.entry.as_ref()),
                        length,
                        |entry| {
                            entry
                                .updated
                                .as_ref()
                                .map_or(built.epoch, |date| date.epoch)
                        },
                    ),
                })
            },
        );
        let rss = self.clone();
        let stylesheet_owner = rss.instances.iter().position(|instance| {
            instance.config.rss_feed_enabled
                && instance.config.stylesheet == "auto"
        });
        ranked.flat_map(move |ranked: &Ranked| {
            feed::artifacts(
                &rss.project,
                &rss.docs,
                &rss.instances[ranked.instance].config,
                ranked.instance,
                stylesheet_owner == Some(ranked.instance),
                &ranked.created,
                &ranked.updated,
                &ranked.built,
            )
        })
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Entry {}
impl Value for Candidate {}
impl Value for Ranked {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Selects at most `length` entries in descending date order.
fn top<'a>(
    entries: impl Iterator<Item = &'a Entry>, length: usize,
    date: impl Fn(&Entry) -> i64,
) -> Vec<Entry> {
    let mut selected = Vec::with_capacity(length.min(64));
    for entry in entries {
        if length == 0 {
            break;
        }
        let timestamp = date(entry);
        let source = entry.page.source().as_str();
        let position = selected
            .iter()
            .position(|existing: &&Entry| {
                // MkDocs' stable sort keeps ascending source order on ties.
                let existing_timestamp = date(existing);
                timestamp > existing_timestamp
                    || (timestamp == existing_timestamp
                        && source < existing.page.source().as_str())
            })
            .unwrap_or(selected.len());
        if position < length {
            selected.insert(position, entry);
            selected.truncate(length);
        }
    }
    selected.into_iter().cloned().collect()
}

/// Identifies one seed, page contribution, or generated feed artifact.
fn candidate_key(provider: &str, instance: usize, source: &str) -> Key<Id> {
    Key::from(
        Id::builder()
            .provider(provider)
            .context(instance.to_string())
            .location(source)
            .build()
            .expect("source and instance identities are valid"),
    )
}

/// Identifies the reduction group for one RSS instance.
fn instance_key(instance: usize) -> Result<Key<Id>> {
    Ok(Key::from(
        Id::builder()
            .provider("rss")
            .context("instance")
            .location(instance.to_string())
            .build()?,
    ))
}
