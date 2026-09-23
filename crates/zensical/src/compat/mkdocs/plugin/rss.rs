// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

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
use crate::structure::page::{Page, PageDescriptor, PageOrigin};
use crate::workflow::{output::Artifact, Configuration};

mod date;
mod format;

use date::{git_dates, resolve, Stamp};

/// RSS plugin and its compiled instance filters.
#[derive(Clone, Debug)]
pub struct Rss {
    instances: Arc<Vec<Instance>>,
    project: Arc<Project>,
    docs: SourceRoot,
    now: Stamp,
}

#[derive(Clone, Debug)]
struct Instance {
    config: RssPluginConfig,
    path: Result<Regex, String>,
}

/// One page's feed contribution for one instance.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    page: Page,
    body: Arc<str>,
    social_image: Option<(String, u64)>,
    created: Stamp,
    updated: Stamp,
}

/// A seed keeps an empty feed alive when no pages match.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    instance: usize,
    entry: Option<Entry>,
}

/// The only global ordering boundary: two bounded top-k selections.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ranked {
    instance: usize,
    created: Vec<Entry>,
    updated: Vec<Entry>,
}

impl Value for Entry {}
impl Value for Candidate {}
impl Value for Ranked {}

impl Rss {
    /// Prepares enabled instances and their path filters.
    pub fn new(config: &Config) -> Self {
        let instances = config
            .project
            .plugins
            .rss
            .config
            .iter()
            .filter(|instance| instance.config.enabled)
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
            now: Stamp::now(),
        }
    }

    /// Produces feed artifacts from final pages and their original Markdown.
    pub fn setup(
        &self, pages: &Stream<Id, Page>,
        descriptors: &Stream<Id, PageDescriptor>,
        configuration: &Stream<Id, Configuration>,
    ) -> Stream<Id, Artifact> {
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
                if matches!(
                    page.meta.get("draft"),
                    Some(crate::structure::dynamic::Dynamic::Bool(true))
                ) {
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
                            &rss.now,
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
                            &rss.now,
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
                                    social_image: None,
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
                let Some(instance) =
                    candidates.values().next().map(|item| item.instance)
                else {
                    return None;
                };
                let length = limits[instance].config.length;
                Some(Ranked {
                    instance,
                    created: top(
                        candidates
                            .values()
                            .filter_map(|item| item.entry.as_ref()),
                        length,
                        |entry| entry.created.epoch,
                    ),
                    updated: top(
                        candidates
                            .values()
                            .filter_map(|item| item.entry.as_ref()),
                        length,
                        |entry| entry.updated.epoch,
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
            format::artifacts(
                &rss.project,
                &rss.docs,
                &rss.instances[ranked.instance].config,
                ranked.instance,
                stylesheet_owner == Some(ranked.instance),
                &ranked.created,
                &ranked.updated,
                &rss.now,
            )
        })
    }
}

fn top<'a>(
    entries: impl Iterator<Item = &'a Entry>, length: usize,
    date: impl Fn(&Entry) -> i64,
) -> Vec<Entry> {
    let mut selected = Vec::with_capacity(length.min(64));
    for entry in entries {
        if length == 0 {
            break;
        }
        let key = (date(entry), entry.page.source().as_str());
        let position = selected
            .iter()
            .position(|existing: &&Entry| {
                key > (date(existing), existing.page.source().as_str())
            })
            .unwrap_or(selected.len());
        if position < length {
            selected.insert(position, entry);
            selected.truncate(length);
        }
    }
    selected.into_iter().cloned().collect()
}

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

fn instance_key(instance: usize) -> Result<Key<Id>> {
    Ok(Key::from(
        Id::builder()
            .provider("rss")
            .context("instance")
            .location(instance.to_string())
            .build()?,
    ))
}
