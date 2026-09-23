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

//! Effective MkDocs resources.

use anyhow::Result;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Stream, Value};

use crate::config::Config;
use crate::path::SitePath;
use crate::watcher::Source;

use super::plugin::meta;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Installs the MkDocs resource pipeline.
#[derive(Debug)]
pub struct Resources {
    /// Immutable classification rules for one workflow lifetime.
    classifier: Classifier,
}

/// Inputs required to derive effective resources.
pub struct Dependencies<'a> {
    /// Physical sources published by the provider.
    pub sources: &'a Stream<Id, Source>,
}

/// One resource after MkDocs source precedence has been resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    /// Original documentation-relative identity before module routing.
    pub source_path: SitePath,
    /// Logical output path relative to the site directory.
    pub path: SitePath,
    /// Physical source path.
    pub source: Source,
    /// Override priority, with lower values taking precedence.
    priority: usize,
}

impl Value for Resource {}

/// Immutable classification rules for one workflow lifetime.
#[derive(Clone, Debug)]
struct Classifier {
    docs: String,
    extra_templates: Vec<String>,
    static_templates: Vec<String>,
    meta: meta::Settings,
    blog_assets: Vec<(String, String)>,
    private_files: Vec<String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Resources {
    /// Resolves the private settings owned by this module instance.
    pub fn new(config: &Config, meta: &meta::Meta) -> Self {
        Self {
            classifier: Classifier::new(config, meta),
        }
    }

    /// Classifies sources and resolves docs-over-theme precedence.
    pub fn setup(
        &self, dependencies: Dependencies<'_>,
    ) -> Stream<Id, Resource> {
        let classifier = self.classifier.clone();
        let resources =
            dependencies
                .sources
                .filter_map(move |id: &Id, source: &Source| {
                    classifier.classify(id, source)
                });

        // Settle precedence before consumers transform or write a resource.
        // Removing a docs override therefore reveals its theme fallback in
        // the same revision without transiently deleting the logical output.
        resources.reduce_by_key(
            |resource: &Resource| resource_key(&resource.path),
            |resources: &dyn Collection<Key<Id>, Resource>| {
                Ok::<_, anyhow::Error>(preferred(
                    resources.iter().map(|(_, resource)| resource),
                ))
            },
        )
    }
}

impl Classifier {
    /// Resolves classification settings once for the workflow lifetime.
    fn new(config: &Config, meta: &meta::Meta) -> Self {
        let blogs = config
            .project
            .plugins
            .blogs
            .config
            .iter()
            .filter(|instance| instance.config.enabled)
            .map(|instance| &instance.config);
        let mut blog_assets = Vec::new();
        let mut private_files = Vec::new();
        for blog in blogs {
            let root = normalize(&blog.blog_dir);
            let posts = normalize(&blog.post_dir.replace("{blog}", &root));
            blog_assets.push((posts, root));
            if blog.authors || blog.authors_profiles {
                private_files.push(normalize(
                    &blog.authors_file.replace("{blog}", &blog.blog_dir),
                ));
            }
        }
        Self {
            docs: config.project.docs_dir.clone(),
            extra_templates: config.project.extra_templates.clone(),
            static_templates: config.project.theme.static_templates.clone(),
            meta: meta.settings().clone(),
            blog_assets,
            private_files,
        }
    }

    /// Classifies one provider source as an MkDocs resource candidate.
    fn classify(&self, id: &Id, source: &Source) -> Result<Option<Resource>> {
        let context = id.context();
        let is_docs = context == self.docs;
        let priority = if is_docs {
            0
        } else {
            let Some(index) = context.strip_prefix("templates/") else {
                return Ok(None);
            };
            index
                .parse::<usize>()
                .ok()
                .and_then(|index| index.checked_add(1))
                .unwrap_or(usize::MAX)
        };
        let mut path = id.location().parse::<SitePath>()?;
        if path.is_hidden() {
            return Ok(None);
        }
        let source_path = path.clone();
        if is_docs {
            if has_extension(&path, "md")
                || meta::claims(path.as_str(), &self.meta)
                || self
                    .private_files
                    .iter()
                    .any(|private| private == path.as_str())
                || self
                    .extra_templates
                    .iter()
                    .any(|item| item == path.as_str())
            {
                return Ok(None);
            }
            path = self.relocate(path)?;
        } else if has_extension(&path, "html")
            || self
                .static_templates
                .iter()
                .any(|item| item == path.as_str())
        {
            return Ok(None);
        }
        Ok(Some(Resource {
            source_path,
            path,
            source: source.clone(),
            priority,
        }))
    }

    fn relocate(&self, path: SitePath) -> Result<SitePath> {
        let matches = self
            .blog_assets
            .iter()
            .filter_map(|(from, to)| {
                path.as_str()
                    .strip_prefix(from)
                    .and_then(|suffix| suffix.strip_prefix('/'))
                    .map(|suffix| (to, suffix))
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Ok(path),
            [(to, suffix)] if to.is_empty() => Ok(suffix.parse()?),
            [(to, suffix)] => Ok(format!("{to}/{suffix}").parse()?),
            _ => anyhow::bail!(
                "resource '{path}' is claimed by multiple blog post directories"
            ),
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Creates the group key used to resolve equivalent resource sources.
fn resource_key(path: &SitePath) -> Result<Key<Id>> {
    let id = Id::builder()
        .provider("asset")
        .context(".")
        .location(path.as_str())
        .build()?;
    Ok(Key::from(id))
}

/// Selects the authoritative source for one logical resource path.
fn preferred<'a>(
    resources: impl Iterator<Item = &'a Resource>,
) -> Option<Resource> {
    resources
        .min_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.source.cmp(&right.source))
        })
        .cloned()
}

/// Returns whether a path has the requested extension, case-insensitively.
fn has_extension(path: &SitePath, expected: &str) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn normalize(path: &str) -> String {
    let path = path.trim_matches('/');
    if matches!(path, "" | ".") {
        String::new()
    } else {
        path.strip_prefix("./").unwrap_or(path).to_owned()
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zrx::id::Id;

    use crate::path::SitePath;
    use crate::watcher::Source;

    use super::{meta, preferred, Classifier, Resource};

    #[test]
    fn classifies_docs_and_ordered_theme_resources() {
        let classifier = classifier();
        let docs = classifier
            .classify(&id("docs", "assets/app.js"), &source("docs/app.js"))
            .unwrap()
            .unwrap();
        let first = classifier
            .classify(
                &id("templates/0", "assets/app.js"),
                &source("theme-0/app.js"),
            )
            .unwrap()
            .unwrap();
        let second = classifier
            .classify(
                &id("templates/1", "assets/app.js"),
                &source("theme-1/app.js"),
            )
            .unwrap()
            .unwrap();

        assert_eq!(docs.priority, 0);
        assert_eq!(first.priority, 1);
        assert_eq!(second.priority, 2);
        assert_eq!(preferred([&second, &first, &docs].into_iter()), Some(docs));
    }

    #[test]
    fn excludes_sources_owned_by_other_mkdocs_stages() {
        let classifier = classifier();
        for path in ["index.md", "GUIDE.MD", ".meta.yml", "extra.txt"] {
            assert!(
                classifier
                    .classify(&id("docs", path), &source(path))
                    .unwrap()
                    .is_none(),
                "{path}"
            );
        }
        for path in ["main.html", "MAIN.HTML", "static.html"] {
            assert!(
                classifier
                    .classify(&id("templates/0", path), &source(path))
                    .unwrap()
                    .is_none(),
                "{path}"
            );
        }
    }

    #[test]
    fn excludes_hidden_resources() {
        let classifier = classifier();
        for (context, path) in [
            ("docs", ".private.yml"),
            ("docs", "section/.nav.yml"),
            ("templates/0", ".icons/logo.svg"),
            ("templates/0", "assets/.private.css"),
        ] {
            assert!(
                classifier
                    .classify(&id(context, path), &source(path))
                    .unwrap()
                    .is_none(),
                "{path}"
            );
        }

        assert!(classifier
            .classify(
                &id("docs", "assets/app.min.js"),
                &source("assets/app.min.js")
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn relocates_blog_assets_and_excludes_author_catalogs() {
        let mut classifier = classifier();
        classifier.blog_assets = vec![("blog/posts".into(), "blog".into())];
        classifier.private_files = vec!["blog/.authors.yml".into()];

        let asset = classifier
            .classify(
                &id("docs", "blog/posts/assets/image.png"),
                &source("image.png"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(asset.path.as_str(), "blog/assets/image.png");
        assert!(
            classifier
                .classify(
                    &id("docs", "blog/.authors.yml"),
                    &source(".authors.yml"),
                )
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn ignores_sources_outside_docs_and_themes() {
        assert!(classifier()
            .classify(&id("site", "asset.js"), &source("site/asset.js"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn uses_source_path_as_a_deterministic_tie_breaker() {
        let left = Resource {
            source_path: "asset.js".parse().unwrap(),
            path: "asset.js".parse().unwrap(),
            source: source("b/asset.js"),
            priority: 1,
        };
        let right = Resource {
            source_path: "asset.js".parse().unwrap(),
            path: "asset.js".parse().unwrap(),
            source: source("a/asset.js"),
            priority: 1,
        };
        assert_eq!(preferred([&left, &right].into_iter()), Some(right));
    }

    #[test]
    fn rejects_unsafe_resource_keys() {
        for path in ["", "/asset.js", "../asset.js", "a/../asset.js"] {
            assert!(path.parse::<SitePath>().is_err(), "{path}");
        }
    }

    fn classifier() -> Classifier {
        Classifier {
            docs: "docs".into(),
            extra_templates: vec!["extra.txt".into()],
            static_templates: vec!["static.html".into()],
            meta: meta::Settings {
                enabled: true,
                meta_file: ".meta.yml".into(),
            },
            blog_assets: Vec::new(),
            private_files: Vec::new(),
        }
    }

    fn id(context: &str, location: &str) -> Id {
        Id::builder()
            .provider("file")
            .context(context)
            .location(location)
            .build()
            .unwrap()
    }

    fn source(path: &str) -> Source {
        Source::from(PathBuf::from(path))
    }
}
