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

//! Material privacy compatibility for HTML and external asset dependencies.

use anyhow::{anyhow, bail, Context as _, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{concurrent, Key, Signal, Stream, Value};

use crate::compat::mkdocs::{resource::Resource, url};
use crate::config::extra::ExtraScript;
use crate::config::plugins::PrivacyPluginConfig;
use crate::config::Config;
use crate::path::{OutputRoot, SitePath};
use crate::structure::page::PageOrigin;
use crate::watcher::Source;
use crate::workflow::output::{self, Artifact};

use super::minify;

mod download;
mod html;
mod media;

use download::{Downloaded, RemoteUrl};

type DownloadCache =
    Arc<Mutex<HashMap<String, Arc<OnceLock<Result<Downloaded, String>>>>>>;

/// Privacy instances and shared download state for one workflow lifetime.
#[derive(Clone, Debug)]
pub struct Privacy {
    instances: Arc<[Instance]>,
    output: OutputRoot,
    concurrency: usize,
}

/// Downloaded assets retained by the document or resource that uses them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assets {
    files: Vec<Asset>,
    preload: BTreeSet<String>,
}

/// One fully patched download and the instance that owns its output.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Asset {
    instance: usize,
    artifact: Artifact,
}

#[derive(Clone, Debug)]
struct ResourceBundle {
    resource: Resource,
    assets: Assets,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Preloads(BTreeSet<String>);

/// Validated settings and deduplicated raw downloads for one instance.
#[derive(Clone, Debug)]
struct Instance {
    id: usize,
    name: String,
    config: PrivacyPluginConfig,
    cache: PathBuf,
    site: String,
    base: String,
    hostname: Option<String>,
    strict: bool,
    agent: ureq::Agent,
    downloads: DownloadCache,
    limit: Arc<download::Limit>,
}

impl Value for Assets {}
impl Value for Asset {}
impl Value for ResourceBundle {}
impl Value for Preloads {}

impl Privacy {
    /// Enabled instances are constructed in configuration order.
    pub fn new(config: &Config, strict: bool) -> Self {
        let mut instances = Vec::new();
        for (id, entry) in
            config.project.plugins.privacy.config.iter().enumerate()
        {
            if !entry.config.enabled {
                continue;
            }
            instances.push(Instance {
                id,
                name: entry.name.clone(),
                config: entry.config.clone(),
                cache: config.project.root_dir.join(&entry.config.cache_dir),
                site: config.project.site_url.clone().unwrap_or_default(),
                base: config.get_base_path(),
                hostname: config.project.site_url.as_deref().and_then(hostname),
                strict,
                agent: ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(5)))
                    .build()
                    .into(),
                downloads: Arc::default(),
                limit: Arc::new(download::Limit::new(entry.config.concurrency)),
            });
        }
        let concurrency = instances
            .iter()
            .map(|instance| instance.config.concurrency)
            .max()
            .unwrap_or(1);
        Self {
            instances: instances.into(),
            output: config.output_root().clone(),
            concurrency,
        }
    }

    /// Local media dependencies are patched before minification and hashing.
    pub fn resources(
        &self, resources: &Stream<Id, Resource>,
    ) -> (Stream<Id, Resource>, Stream<Id, Assets>) {
        if self.instances.is_empty() {
            return (
                resources.clone(),
                resources.map(|_: &Resource| Assets::default()),
            );
        }
        let privacy = self.clone();
        let bundles = resources.map(concurrent(
            self.concurrency,
            move |resource: &Resource| {
                let extension = resource
                    .path
                    .extension()
                    .map(|extension| format!(".{extension}"))
                    .unwrap_or_default();
                let mut resource = resource.clone();
                let mut assets = Assets::default();
                for instance in privacy.instances.iter() {
                    if !instance.config.assets
                        || !instance.has_expression(&extension)
                    {
                        continue;
                    }
                    let content = resource.source.read_to_string()?;
                    let content = content.trim_start_matches('\u{feff}');
                    let links = instance.media_links(content, &extension)?;
                    let graph = instance.graph(
                        links.iter().map(|link| link.value.clone()),
                        resource.path.as_str(),
                    )?;
                    let patched = instance.patch_media(
                        content,
                        &links,
                        &graph,
                        resource.path.as_str(),
                    );
                    if patched != content {
                        resource.source = Source::generated(
                            resource.source.to_path_buf(),
                            patched.into(),
                        );
                    }

                    // Mermaid is preloaded when no site URL is available.
                    if instance.site.is_empty() {
                        for link in &links {
                            if link.value.contains("mermaid.min.js")
                                && let Some(file) =
                                    downloaded(&graph, &link.value)
                            {
                                assets
                                    .preload
                                    .insert(download::public_path(&file.path));
                            }
                        }
                    }
                    instance.emit(&graph, &mut assets)?;
                }
                Ok::<_, anyhow::Error>(ResourceBundle { resource, assets })
            },
        ));
        (
            bundles.map(|bundle: &ResourceBundle| bundle.resource.clone()),
            bundles.map(|bundle: &ResourceBundle| bundle.assets.clone()),
        )
    }

    /// Resource-derived script preloads are exposed to template rendering.
    pub fn project(
        &self, manifest: &Signal<Id, minify::Manifest>,
        assets: &Stream<Id, Assets>,
    ) -> Stream<Id, minify::Manifest> {
        if self.instances.is_empty() {
            return (**manifest).clone();
        }
        let scripts =
            assets.reduce(|assets: &dyn Collection<Key<Id>, Assets>| {
                Some(Preloads(
                    assets
                        .values()
                        .flat_map(|assets| assets.preload.iter().cloned())
                        .collect(),
                ))
            });
        manifest.product(&scripts).map(
            |(manifest, scripts): &(minify::Manifest, Preloads)| {
                let mut manifest = manifest.clone();
                if !scripts.0.is_empty() {
                    let project = Arc::make_mut(&mut manifest.project);
                    for path in &scripts.0 {
                        if !project
                            .extra_javascript
                            .iter()
                            .any(|script| script.path == *path)
                        {
                            project.extra_javascript.push(ExtraScript {
                                path: path.clone(),
                                r#type: None,
                                r#async: false,
                                defer: false,
                            });
                        }
                    }
                    let mut hash = DefaultHasher::new();
                    manifest.hash.hash(&mut hash);
                    project.extra_javascript.hash(&mut hash);
                    manifest.hash = hash.finish();
                }
                manifest
            },
        )
    }

    /// Complete rendered HTML is processed before final HTML minification.
    pub fn html(
        &self, mut content: String, destination: &SitePath,
    ) -> Result<(String, Assets)> {
        let mut assets = Assets::default();
        for instance in self.instances.iter() {
            content = instance.html(&content, destination, &mut assets)?;
        }
        Ok((content, assets))
    }

    /// Shared downloads are retained until their last initiator is removed.
    pub fn setup(&self, assets: &Stream<Id, Assets>) {
        if self.instances.is_empty() {
            return;
        }
        self.setup_outputs(assets);
    }

    fn setup_outputs(&self, assets: &Stream<Id, Assets>) {
        let assets = assets
            .flat_map(|assets: &Assets| {
                assets
                    .files
                    .iter()
                    .map(|asset| {
                        Ok((
                            asset_key(
                                asset.instance,
                                &asset.artifact.destination,
                            )?,
                            asset.clone(),
                        ))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .reduce_by_key(
                |asset: &Asset| asset_key(0, &asset.artifact.destination),
                |assets: &dyn Collection<Key<Id>, Asset>| {
                    assets.values().max_by_key(|asset| asset.instance).cloned()
                },
            )
            .map(|asset: &Asset| asset.artifact.clone());
        output::setup(self.output.clone(), &assets);
    }
}

impl Instance {
    /// Material's hostname and include-before-exclude rules are applied.
    fn accepts(&self, value: &str, initiator: &str) -> Result<bool> {
        if !self.config.assets
            || hostname(value).is_none()
            || hostname(value) == self.hostname
        {
            return Ok(false);
        }
        let remote = match RemoteUrl::parse(value) {
            Ok(remote) => remote,
            Err(error) => {
                self.report(
                    "warn",
                    &format!(
                        "Couldn't process {value} in '{initiator}': {error:#}"
                    ),
                )?;
                return Ok(false);
            }
        };
        // The original plugin returns immediately for inclusion patterns,
        // before exclusions and the report-only switch are evaluated.
        if !self.config.assets_include.is_empty() {
            return media::matches(&remote.key, &self.config.assets_include);
        }
        if media::matches(&remote.key, &self.config.assets_exclude)? {
            return Ok(false);
        }
        if !self.config.assets_fetch {
            self.report(
                "warn",
                &format!("External file: {value} in '{initiator}'"),
            )?;
            return Ok(false);
        }
        Ok(true)
    }

    /// Successful raw downloads and their transitive dependencies are collected.
    fn graph(
        &self, urls: impl IntoIterator<Item = String>, initiator: &str,
    ) -> Result<BTreeMap<String, Downloaded>> {
        let mut pending = urls
            .into_iter()
            .map(|url| (url, initiator.to_owned()))
            .collect::<VecDeque<_>>();
        let mut seen = BTreeSet::new();
        let mut graph = BTreeMap::new();
        while let Some((value, via)) = pending.pop_front() {
            if !self.accepts(&value, &via)? {
                continue;
            }
            let remote = RemoteUrl::parse(&value)?;
            if !seen.insert(remote.key.clone()) {
                continue;
            }
            let cell = self
                .downloads
                .lock()
                .map_err(|_| anyhow!("privacy download cache was poisoned"))?
                .entry(remote.key.clone())
                .or_default()
                .clone();
            let file = match cell.get_or_init(|| {
                self.fetch(&remote).map_err(|error| format!("{error:#}"))
            }) {
                Ok(file) => file.clone(),
                Err(error) => {
                    self.report(
                        "warn",
                        &format!(
                            "Couldn't retrieve {value} in '{via}': {error}"
                        ),
                    )?;
                    continue;
                }
            };
            let extension = file.extension();
            if self.has_expression(&extension) {
                let content = std::str::from_utf8(&file.bytes)
                    .context("external media is not UTF-8")?
                    .trim_start_matches('\u{feff}');
                for link in self.media_links(content, &extension)? {
                    pending.push_back((link.value, file.path.to_string()));
                }
            }
            graph.insert(remote.key, file);
        }
        Ok(graph)
    }

    /// Only successful dependencies are substituted, so failed URLs stay usable.
    fn patch_media(
        &self, content: &str, links: &[media::Link],
        graph: &BTreeMap<String, Downloaded>, initiator: &str,
    ) -> String {
        let mut output = String::new();
        let mut cursor = 0;
        let javascript = Path::new(initiator)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("js"));
        for link in links {
            let Some(file) = downloaded(graph, &link.value) else {
                continue;
            };
            if link.start < cursor {
                continue;
            }
            output.push_str(&content[cursor..link.start]);
            let path = download::public_path(&file.path);
            let replacement = if javascript {
                // Dynamic DOM URLs are resolved against the page, so the
                // site's base path is used for every JavaScript dependency.
                // The current origin is retained for local previews.
                format!("{}/{path}", self.base.trim_end_matches('/'))
            } else {
                let relative = url::relative(initiator, &path);
                // A self-reference must be resolved to the file, not its
                // containing directory as implied by the URL `.`.
                if relative == "." {
                    path.rsplit('/').next().unwrap_or(&path).to_owned()
                } else {
                    relative
                }
            };
            output.push_str(&replacement);
            if let Some(fragment) =
                link.value.split_once('#').map(|(_, fragment)| fragment)
            {
                output.push('#');
                output.push_str(fragment);
            }
            cursor = link.end;
        }
        output.push_str(&content[cursor..]);
        output
    }

    /// Patched asset bytes are emitted separately from the persistent raw cache.
    fn emit(
        &self, graph: &BTreeMap<String, Downloaded>, assets: &mut Assets,
    ) -> Result<()> {
        for file in graph.values() {
            let extension = file.extension();
            let contents = if self.has_expression(&extension) {
                // Patched bytes are shared across all initiators, while the
                // persistent cache remains reusable under different settings.
                file.patched
                    .get_or_init(|| {
                        (|| -> Result<_> {
                            let content = std::str::from_utf8(&file.bytes)?
                                .trim_start_matches('\u{feff}');
                            Ok(Arc::new(
                                self.patch_media(
                                    content,
                                    &self.media_links(content, &extension)?,
                                    graph,
                                    file.path.as_str(),
                                )
                                .into_bytes(),
                            ))
                        })()
                        .map_err(|error| format!("{error:#}"))
                    })
                    .as_ref()
                    .map_err(|error| anyhow!("{error}"))?
                    .clone()
            } else {
                file.bytes.clone()
            };
            assets.files.push(Asset {
                instance: self.id,
                artifact: Artifact {
                    owner: PageOrigin::Generated {
                        identity: format!("privacy:{}:{}", self.id, file.path),
                        provenance: None,
                    },
                    destination: file.path.clone(),
                    contents,
                },
            });
        }
        Ok(())
    }

    /// Diagnostics are filtered using Material's log settings and strict mode.
    fn report(&self, level: &str, message: &str) -> Result<()> {
        let rank = |level| match level {
            "error" => 3,
            "warn" => 2,
            "info" => 1,
            _ => 0,
        };
        if !self.config.log || rank(level) < rank(&self.config.log_level) {
            return Ok(());
        }
        let label = if level == "warn" { "WARNING" } else { "INFO" };
        eprintln!("{label} -  [{}] {message}", self.name);
        if level == "warn" && self.strict {
            bail!("Aborted because --strict flag is set")
        }
        Ok(())
    }
}

fn hostname(value: &str) -> Option<String> {
    let uri = download::parse_url(value)?;
    if uri.scheme().is_some_and(|scheme| {
        !matches!(
            scheme.as_str().to_ascii_lowercase().as_str(),
            "http" | "https"
        )
    }) {
        return None;
    }
    uri.authority()
        .map(|authority| authority.host().to_ascii_lowercase())
}

fn downloaded<'a>(
    graph: &'a BTreeMap<String, Downloaded>, value: &str,
) -> Option<&'a Downloaded> {
    graph.get(&RemoteUrl::parse(value).ok()?.key)
}

fn asset_key(instance: usize, path: &SitePath) -> Result<Key<Id>> {
    Ok(Key::from(
        Id::builder()
            .provider("privacy")
            .context(format!("instance/{instance}"))
            .location(path.as_str())
            .build()?,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use zrx::id::Id;
    use zrx::stream::{Key, Workflow};

    use super::{Asset, Assets, Privacy};
    use crate::path::OutputRoot;
    use crate::structure::page::PageOrigin;
    use crate::workflow::output::Artifact;

    fn document(name: &str) -> Key<Id> {
        Key::from(
            Id::builder()
                .provider("test")
                .context("docs")
                .location(name)
                .build()
                .unwrap(),
        )
    }

    fn shared(instance: usize, content: &str) -> Assets {
        Assets {
            files: vec![Asset {
                instance,
                artifact: Artifact::page(
                    PageOrigin::Generated {
                        identity: format!("privacy:{instance}"),
                        provenance: None,
                    },
                    "assets/external/shared.png".parse().unwrap(),
                    content.as_bytes().to_vec(),
                ),
            }],
            ..Assets::default()
        }
    }

    #[test]
    fn shared_downloads_survive_partial_removal_and_instance_handoffs() {
        let directory = tempfile::tempdir().unwrap();
        let privacy = Privacy {
            // Output reconciliation is exercised without making HTTP requests.
            instances: Arc::from([]),
            output: OutputRoot::prepare(directory.path()).unwrap(),
            concurrency: 1,
        };
        let workflow = Workflow::<Id>::build(|workflow| {
            let assets = workflow.input::<Assets>();
            privacy.setup_outputs(&assets);
        });
        let mut runner = workflow.runner().unwrap();
        let input = runner.input::<Assets>().unwrap();
        let path = directory.path().join("assets/external/shared.png");

        // The same download is retained by two independent source documents.
        let mut revision = input.begin().unwrap();
        revision
            .insert(document("first"), shared(0, "first instance"))
            .unwrap();
        revision
            .insert(document("second"), shared(0, "first instance"))
            .unwrap();
        let mut input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first instance");

        // Removing one initiator must not remove a shared output.
        let mut revision = input.begin().unwrap();
        revision.remove(document("first")).unwrap();
        input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert!(path.exists());

        // A later instance wins, then yields to the remaining earlier owner.
        let mut revision = input.begin().unwrap();
        revision
            .insert(document("later"), shared(1, "last instance"))
            .unwrap();
        input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"last instance");

        let mut revision = input.begin().unwrap();
        revision.remove(document("later")).unwrap();
        input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first instance");

        // The file is removed only after the final initiator is retracted.
        let mut revision = input.begin().unwrap();
        revision.remove(document("second")).unwrap();
        let _input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert!(!path.exists());
    }
}
