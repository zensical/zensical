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

//! Native compatibility pipeline for filesystem-backed literate navigation.

use anyhow::Context;
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal, Stream, Value};

use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::nav::{Navigation, NavigationItem, NavigationResolution};
use crate::structure::page::Page;
use crate::watcher::Source;

mod parser;
mod resolver;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Literate navigation pipeline.
#[derive(Clone, Debug)]
pub struct LiterateNav {
    settings: Arc<Settings>,
}

/// Inputs required to derive revision-complete navigation.
pub struct Dependencies<'a> {
    /// Physical sources, including navigation control files.
    pub sources: &'a Stream<Id, Source>,
    /// Rendered documentation pages.
    pub pages: &'a Stream<Id, Page>,
}

/// Immutable native plugin settings.
#[derive(Clone, Debug)]
struct Settings {
    /// Whether native literate navigation participates in resolution.
    enabled: bool,
    /// Provider context containing documentation sources.
    docs: String,
    /// Folder-relative navigation document name or path.
    nav_file: String,
    /// Whether a folder index omitted from its list is inserted first.
    implicit_index: bool,
    /// Explicit MkDocs navigation used as the root fallback or seed.
    configured: Vec<NavigationItem>,
}

/// One discovered literate navigation document.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Document {
    /// Canonical source-relative navigation document path.
    path: SourcePath,
    /// Complete Markdown source after optional byte-order-mark removal.
    content: String,
}

impl Value for Document {}

/// Revision-complete navigation documents.
#[derive(Clone, Debug, Default)]
struct Documents(
    /// Documents keyed by canonical source-relative path.
    Arc<BTreeMap<String, String>>,
);

impl Value for Documents {}

/// Revision-complete rendered pages.
#[derive(Clone, Debug)]
struct Pages(
    /// Rendered pages in the settled workflow revision.
    Arc<Vec<Page>>,
);

impl Value for Pages {}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl LiterateNav {
    /// Resolves plugin settings for one workflow lifetime.
    pub fn new(config: &Config) -> Self {
        let plugin = &config.project.plugins.literate_nav.config;
        Self {
            settings: Arc::new(Settings {
                enabled: plugin.enabled,
                docs: config.project.docs_dir.clone(),
                nav_file: plugin.nav_file.clone(),
                implicit_index: plugin.implicit_index,
                configured: config.project.nav.clone(),
            }),
        }
    }

    /// Installs navigation discovery, settlement, and compilation.
    pub fn setup(
        &self, dependencies: Dependencies<'_>,
    ) -> Signal<Id, NavigationResolution> {
        let settings = self.settings.clone();
        let documents = dependencies.sources.filter_map({
            let settings = settings.clone();
            move |id: &Id, source: &Source| {
                if !settings.enabled || id.context() != settings.docs {
                    return Ok(None);
                }
                let path = id.location().parse::<SourcePath>()?;
                if !is_navigation_file(&path, &settings.nav_file) {
                    return Ok(None);
                }
                let content =
                    fs::read_to_string(&**source).with_context(|| {
                        format!(
                            "failed to read literate navigation file {path}"
                        )
                    })?;
                Ok::<_, anyhow::Error>(Some(Document {
                    path,
                    content: content.trim_start_matches('\u{feff}').into(),
                }))
            }
        });
        let documents = documents.reduce(
            |documents: &dyn Collection<Key<Id>, Document>| {
                Some(Documents(Arc::new(
                    documents
                        .values()
                        .map(|document| {
                            (
                                document.path.to_string(),
                                document.content.clone(),
                            )
                        })
                        .collect(),
                )))
            },
        );
        let pages = dependencies.pages.reduce(
            |pages: &dyn Collection<Key<Id>, Page>| {
                Some(Pages(Arc::new(pages.values().cloned().collect())))
            },
        );

        let navigation = pages.product(&documents).map(
            move |pages: &Pages, docs: &Documents| {
                if settings.enabled {
                    resolver::resolve(&settings, &docs.0, pages.0.as_ref())
                } else {
                    Ok(Navigation::resolve(
                        settings.configured.clone(),
                        pages.0.as_ref().clone(),
                    ))
                }
            },
        );
        navigation.reduce(
            |navigation: &dyn Collection<Key<Id>, NavigationResolution>| {
                navigation.values().next().cloned()
            },
        )
    }
}

/// Returns whether a source can serve as a folder navigation document.
fn is_navigation_file(path: &SourcePath, nav_file: &str) -> bool {
    path.as_str() == nav_file
        || path
            .as_str()
            .strip_suffix(nav_file)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::is_navigation_file;

    #[test]
    fn discovers_simple_and_nested_navigation_names() {
        assert!(is_navigation_file(
            &"guide/SUMMARY.md".parse().unwrap(),
            "SUMMARY.md"
        ));
        assert!(is_navigation_file(
            &"guide/nav/SUMMARY.md".parse().unwrap(),
            "nav/SUMMARY.md"
        ));
        assert!(!is_navigation_file(
            &"guide/NOT-SUMMARY.md".parse().unwrap(),
            "SUMMARY.md"
        ));
    }
}
