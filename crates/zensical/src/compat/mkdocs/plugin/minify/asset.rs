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

//! External asset processing for the MkDocs-compatible minify plugin.

use anyhow::{anyhow, Context as _};
use sha2::{Digest, Sha384};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal, Stream, Value};

use crate::compat::mkdocs::resource::Resource;
use crate::config::plugins::MinifyPluginConfig;
use crate::config::Project;
use crate::path::SitePath;
use crate::watcher::Source;

use super::{script, style, Minify};

mod selector;
mod writer;

use selector::{normalize, Selector};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Supported external asset kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    JavaScript,
    Stylesheet,
}

/// Final asset bytes or a source file that can be copied without reading it.
#[derive(Clone, Debug)]
enum Contents {
    Copy(Source),
    Bytes(Arc<[u8]>),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One fully resolved output asset.
#[derive(Clone, Debug)]
struct Emission {
    /// Original site-relative resource path.
    source_path: SitePath,
    /// Final site-relative path after hashing and minification.
    output_path: SitePath,
    /// Bytes to write or source file to copy.
    contents: Contents,
    /// Whether this plugin transformed and owns the resource.
    claimed: bool,
}

/// Path rewrite produced by one claimed asset.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mapping {
    /// Original configured asset path.
    source_path: SitePath,
    /// Final emitted asset path.
    output_path: SitePath,
}

/// Asset-derived project view consumed by template rendering.
#[derive(Clone, Debug)]
pub struct Manifest {
    /// Project with configured asset paths rewritten to their emitted names.
    pub project: Arc<Project>,
    /// Stable hash of the current path mapping.
    pub hash: u64,
}

/// Compiled settings for external asset selection and transformation.
#[derive(Clone, Debug)]
struct Settings {
    /// Whether the compatibility plugin is enabled.
    enabled: bool,
    /// JavaScript resources claimed by the plugin.
    javascript: Selector,
    /// Stylesheet resources claimed by the plugin.
    stylesheet: Selector,
    /// Asset kinds whose contents should be minified.
    minify: BTreeSet<Kind>,
    /// Whether transformed names include a content digest.
    cache_safe: bool,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Settings {
    /// Compiles asset settings once for the workflow.
    fn new(config: &MinifyPluginConfig) -> Self {
        Self {
            enabled: config.enabled,
            javascript: Selector::new(&config.js_files),
            stylesheet: Selector::new(&config.css_files),
            minify: [
                config.minify_js.then_some(Kind::JavaScript),
                config.minify_css.then_some(Kind::Stylesheet),
            ]
            .into_iter()
            .flatten()
            .collect(),
            cache_safe: config.cache_safe,
        }
    }

    /// Classifies one resource, rejecting conflicting selectors.
    fn claim(&self, path: &SitePath) -> anyhow::Result<Option<Kind>> {
        if !self.active() {
            return Ok(None);
        }
        let javascript = self.active_kind(Kind::JavaScript)
            && self.javascript.matches(path.as_str())?;
        let stylesheet = self.active_kind(Kind::Stylesheet)
            && self.stylesheet.matches(path.as_str())?;
        match (javascript, stylesheet) {
            (true, true) => Err(anyhow!(
                "asset is selected as both JavaScript and CSS: {path}"
            )),
            (true, false) => {
                ensure_extension(path, "js")?;
                Ok(Some(Kind::JavaScript))
            }
            (false, true) => {
                ensure_extension(path, "css")?;
                Ok(Some(Kind::Stylesheet))
            }
            (false, false) => Ok(None),
        }
    }

    /// Produces the effective output for one resource.
    fn transform(&self, resource: &Resource) -> anyhow::Result<Emission> {
        let Some(kind) = self.claim(&resource.path)? else {
            return Ok(Emission {
                source_path: resource.path.clone(),
                output_path: resource.path.clone(),
                contents: Contents::Copy(resource.source.clone()),
                claimed: false,
            });
        };

        let source =
            fs::read_to_string(&resource.source).with_context(|| {
                format!("failed to read asset {}", resource.source.display())
            })?;
        let minify = self.minify.contains(&kind);
        let transformed = if minify {
            match kind {
                Kind::JavaScript => script::minify(&source, false),
                Kind::Stylesheet => style::minify(&source),
            }
            .filter(|output| output.len() < source.len())
            .unwrap_or(source)
        } else {
            source
        };
        let bytes = Arc::<[u8]>::from(transformed.into_bytes());
        let output_path = output_path(
            &resource.path,
            minify,
            self.cache_safe.then(|| digest(&bytes)),
        )?;
        Ok(Emission {
            source_path: resource.path.clone(),
            output_path,
            contents: Contents::Bytes(bytes),
            claimed: true,
        })
    }

    /// Validates exact selectors against the settled claimed relation.
    fn validate<'a>(
        &self, mappings: impl Iterator<Item = &'a Mapping>,
    ) -> anyhow::Result<()> {
        if !self.active() {
            return Ok(());
        }
        for (kind, selectors) in [
            (Kind::JavaScript, &self.javascript),
            (Kind::Stylesheet, &self.stylesheet),
        ] {
            if !self.active_kind(kind) {
                continue;
            }
            if let Some(error) = selectors.error() {
                return Err(anyhow!("invalid minify asset selector: {error}"));
            }
        }
        let found = mappings
            .map(|mapping| mapping.source_path.as_str())
            .collect::<BTreeSet<_>>();
        let missing = [
            (Kind::JavaScript, &self.javascript),
            (Kind::Stylesheet, &self.stylesheet),
        ]
        .into_iter()
        .filter(|(kind, _)| self.active_kind(*kind))
        .flat_map(|(_, selector)| selector.exact())
        .filter(|path| !found.contains(path.as_str()))
        .collect::<Vec<_>>();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(
                "selected asset does not exist: {}",
                missing
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }

    /// Returns whether any transformation mode is active.
    fn active(&self) -> bool {
        self.active_kind(Kind::JavaScript) || self.active_kind(Kind::Stylesheet)
    }

    /// Returns whether one asset kind participates in this build.
    fn active_kind(&self, kind: Kind) -> bool {
        if !self.enabled || (!self.minify.contains(&kind) && !self.cache_safe) {
            return false;
        }
        match kind {
            Kind::JavaScript => !self.javascript.is_empty(),
            Kind::Stylesheet => !self.stylesheet.is_empty(),
        }
    }
}

impl Manifest {
    /// Projects emitted asset names into template-visible project settings.
    fn new<'a>(
        project: Arc<Project>, mappings: impl Iterator<Item = &'a Mapping>,
    ) -> Self {
        let paths = mappings
            .map(|mapping| {
                (
                    mapping.source_path.to_string(),
                    mapping.output_path.to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if paths.is_empty() {
            return Self { project, hash: 0 };
        }

        let mut projected = (*project).clone();
        projected.extra_css = projected
            .extra_css
            .iter()
            .map(|path| rewrite(path, &paths))
            .collect();
        projected.extra_javascript = projected
            .extra_javascript
            .iter()
            .cloned()
            .map(|mut script| {
                script.path = rewrite(&script.path, &paths);
                script
            })
            .collect();
        let changed = projected.extra_css != project.extra_css
            || projected
                .extra_javascript
                .iter()
                .map(|script| &script.path)
                .ne(project.extra_javascript.iter().map(|script| &script.path));
        if !changed {
            return Self { project, hash: 0 };
        }
        let mut hasher = DefaultHasher::new();
        projected.extra_css.hash(&mut hasher);
        projected.extra_javascript.hash(&mut hasher);
        Self {
            project: Arc::new(projected),
            hash: hasher.finish(),
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Emission {}

// ----------------------------------------------------------------------------

impl Value for Mapping {}

// ----------------------------------------------------------------------------

impl Value for Manifest {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Transforms selected resources, writes every effective asset, and publishes
/// the project view whose configured asset paths name the emitted files.
pub fn setup(
    plugin: &Minify, resources: &Stream<Id, Resource>,
) -> Signal<Id, Manifest> {
    let settings = Settings::new(plugin.config());
    let settings_for_transform = settings.clone();
    let emissions = resources.map(move |resource: &Resource| {
        settings_for_transform.transform(resource)
    });

    let outputs = emissions.unique_by_key(move |emission: &Emission| {
        output_key(&emission.output_path)
    });
    writer::setup(plugin.output().clone(), &outputs);

    let project = plugin.project().clone();
    let settings_for_manifest = settings;
    outputs
        .filter(|emission: &Emission| emission.claimed)
        .map(|emission: &Emission| Mapping {
            source_path: emission.source_path.clone(),
            output_path: emission.output_path.clone(),
        })
        .reduce(move |mappings: &dyn Collection<Key<Id>, Mapping>| {
            settings_for_manifest
                .validate(mappings.iter().map(|(_, mapping)| mapping))?;
            Ok::<_, anyhow::Error>(Some(Manifest::new(
                project.clone(),
                mappings.iter().map(|(_, mapping)| mapping),
            )))
        })
}

fn output_key(path: &SitePath) -> anyhow::Result<Key<Id>> {
    let id = Id::builder()
        .provider("file")
        .context(".")
        .location(path.as_str())
        .build()?;
    Ok(Key::from(id))
}

fn output_path(
    path: &SitePath, minify: bool, digest: Option<String>,
) -> anyhow::Result<SitePath> {
    let extension = path
        .extension()
        .ok_or_else(|| anyhow!("selected asset has no extension: {path}"))?;
    let mut name = path.file_stem().to_owned();
    if let Some(digest) = digest {
        name.push('.');
        name.push_str(&digest[..6]);
    }
    if minify {
        name.push_str(".min");
    }
    name.push('.');
    name.push_str(extension);
    Ok(path.with_file_name(&name)?)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha384::digest(bytes))
}

fn rewrite(path: &str, mappings: &BTreeMap<String, String>) -> String {
    let normalized = normalize(path);
    mappings
        .get(&normalized)
        .cloned()
        .unwrap_or_else(|| path.to_owned())
}

fn ensure_extension(path: &SitePath, expected: &str) -> anyhow::Result<()> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "selected asset must have a .{expected} extension: {path}"
        ))
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::path::SitePath;

    use super::{digest, output_key, output_path, rewrite};

    #[test]
    fn output_names_follow_minify_and_cache_safe_modes() {
        assert_eq!(
            output_path(&"assets/app.js".parse().unwrap(), true, None).unwrap(),
            "assets/app.min.js".parse::<SitePath>().unwrap()
        );
        assert_eq!(
            output_path(
                &"assets/app.js".parse().unwrap(),
                false,
                Some("abcdef12".into())
            )
            .unwrap(),
            "assets/app.abcdef.js".parse::<SitePath>().unwrap()
        );
        assert_eq!(
            output_path(
                &"assets/app.js".parse().unwrap(),
                true,
                Some("abcdef12".into())
            )
            .unwrap(),
            "assets/app.abcdef.min.js".parse::<SitePath>().unwrap()
        );
    }

    #[test]
    fn output_keys_contain_only_site_relative_identity() {
        let key = output_key(&"assets/café.js".parse().unwrap()).unwrap();
        let id = key.try_as_id().unwrap();

        assert_eq!(id.context(), ".");
        assert_eq!(id.location(), "assets/café.js");
        assert!("../outside.js".parse::<SitePath>().is_err());
    }

    #[test]
    fn digest_uses_sha384_and_emitted_bytes() {
        assert_eq!(&digest(b"const value=1;")[..6], "11c998");
    }

    #[test]
    fn rewrite_preserves_unmatched_and_structured_paths() {
        let paths = BTreeMap::from([(
            "assets/app.js".into(),
            "assets/app.abcdef.min.js".into(),
        )]);
        assert_eq!(
            rewrite("./assets/app.js", &paths),
            "assets/app.abcdef.min.js"
        );
        assert_eq!(
            rewrite("https://example.com/app.js", &paths),
            "https://example.com/app.js"
        );
    }
}
