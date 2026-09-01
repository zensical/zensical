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

//! MkDocs-compatible search plugin.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::sync::Arc;
use zrx::id::Id;
use zrx::scheduler::Value;
use zrx::stream::{Key, Signal};

use crate::config::plugins::SearchPluginConfig;
use crate::config::Config;
use crate::structure::dynamic::Dynamic;
use crate::structure::nav::{file_sort_key, Navigation};
use crate::structure::page::Page;

mod item;
mod parser;

use item::{SearchItem, SearchSection};
use parser::Parser;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Search configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SearchConfig {
    /// Languages for tokenizer.
    lang: Vec<String>,
    /// Separator for tokenizer.
    separator: String,
}

/// Complete search artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SearchIndex {
    /// Search configuration.
    config: SearchConfig,
    /// Search items.
    items: Vec<SearchItem>,
}

/// Search facts extracted while rendering one Markdown page.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Facts {
    /// Page-local search sections.
    sections: Vec<SearchSection>,
}

/// Compact page document retained by the search branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Document {
    /// Page target URL.
    url: String,
    /// Page title.
    title: String,
    /// Page tag names.
    tags: Vec<String>,
    /// Page-local facts extracted before page construction.
    facts: Arc<Facts>,
}

/// Revision-aligned search inputs from the site settlement boundary.
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    /// Compact page documents.
    documents: Arc<Vec<(Key<Id>, Document)>>,
    /// Navigation from the same page revision.
    nav: Navigation,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl SearchConfig {
    /// Creates search configuration for the configured theme language.
    fn new(config: SearchPluginConfig, language: &str) -> Self {
        Self {
            lang: vec![language.to_string()],
            separator: config.separator,
        }
    }
}

// ----------------------------------------------------------------------------

impl Document {
    /// Attaches page properties to previously extracted search facts.
    pub(crate) fn new(page: &Page, facts: Arc<Facts>) -> Self {
        Self {
            url: page.url.clone(),
            title: page.title.clone(),
            tags: page.tags().into_iter().map(|tag| tag.name).collect(),
            facts,
        }
    }
}

// ----------------------------------------------------------------------------

impl Snapshot {
    /// Creates a search snapshot without another site-wide reduction.
    pub(crate) fn new(
        documents: Vec<(Key<Id>, Document)>, nav: Navigation,
    ) -> Self {
        Self {
            documents: Arc::new(documents),
            nav,
        }
    }
}

// ----------------------------------------------------------------------------

impl Facts {
    /// Returns whether this page contributes anything to the search index.
    pub(crate) fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

// ----------------------------------------------------------------------------

impl SearchIndex {
    /// Creates a search index from compact page facts.
    #[allow(clippy::assigning_clones)]
    fn new(
        documents: Vec<(Key<Id>, Document)>, nav: &Navigation,
        config: SearchPluginConfig, language: &str,
    ) -> Self {
        let mut items: Vec<SearchItem> = Vec::new();

        let mut documents = Vec::from_iter(documents);
        documents.sort_by_key(|(id, _)| file_sort_key(&id[0]));

        // Attach site-wide navigation facts only while assembling the final
        // artifact, keeping them out of each page-local stream value.
        for (_id, document) in documents {
            let iter = nav.ancestors_for_url(&document.url).into_iter().rev();
            let mut path = iter
                .filter_map(|item| {
                    item.display_title().map(ToString::to_string)
                })
                .collect::<Vec<_>>();

            // Add page title to path if not already present - this might be
            // the true in case of index pages
            if path.last() != Some(&document.title) {
                path.push(document.title.clone());
            }

            for section in &document.facts.sections {
                let location = match &section.location {
                    Some(id) => format!("{}#{}", document.url, id),
                    _ => document.url.clone(),
                };
                let title = if section.title.is_empty() {
                    document.title.clone()
                } else {
                    section.title.clone()
                };
                items.push(SearchItem {
                    location: Some(location),
                    level: section.level,
                    title,
                    text: section.text.clone(),
                    path: path.clone(),
                    tags: document.tags.clone(),
                });
            }
        }

        // Return search
        Self {
            config: SearchConfig::new(config, language),
            items,
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Document {}
impl Value for Snapshot {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Attach MkDocs-compatible search artifact generation to the build graph.
pub(crate) fn attach(config: &Config, snapshot: &Signal<Id, Snapshot>) {
    let config = config.clone();
    let _ = snapshot.map(move |snapshot: &Snapshot| {
        let documents = if config.project.plugins.search.config.enabled {
            snapshot.documents.as_ref().clone()
        } else {
            Vec::new()
        };
        let search = SearchIndex::new(
            documents,
            &snapshot.nav,
            config.project.plugins.search.config.clone(),
            &config.project.theme.language,
        );
        write(&config, &search)
    });
}

/// Creates the page-local search visitor.
pub(crate) fn parser(meta: &BTreeMap<String, Dynamic>) -> Parser {
    if is_search_excluded(meta) {
        Parser::discarding()
    } else {
        Parser::default()
    }
}

/// Converts a completed visitor into cached page-local facts.
pub(crate) fn finish(parser: Parser) -> Arc<Facts> {
    Arc::new(Facts { sections: parser.finish() })
}

/// Write search artifacts without retaining a second serialized copy.
fn write(config: &Config, search: &SearchIndex) -> anyhow::Result<()> {
    let site_dir = config.get_site_dir();
    let path = site_dir.join("search.json");
    fs::create_dir_all(path.parent().expect("invariant"))?;
    let mut writer = BufWriter::new(fs::File::create(path)?);
    serde_json::to_writer(&mut writer, search)?;
    writer.flush()?;

    if config.project.plugins.offline.config.enabled {
        let path = site_dir.join("search.js");
        fs::create_dir_all(path.parent().expect("invariant"))?;
        let mut writer = BufWriter::new(fs::File::create(path)?);
        writer.write_all(b"var __index = ")?;
        serde_json::to_writer(&mut writer, search)?;
        writer.write_all(b";")?;
        writer.flush()?;
    }
    Ok(())
}

/// Returns whether a page is excluded from search through its metadata.
fn is_search_excluded(meta: &BTreeMap<String, Dynamic>) -> bool {
    let Some(Dynamic::Map(search)) = meta.get("search") else {
        return false;
    };
    matches!(search.get("exclude"), Some(Dynamic::Bool(true)))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_exclusion_is_read_from_page_metadata() {
        let mut search = BTreeMap::new();
        search.insert(String::from("exclude"), Dynamic::Bool(true));
        let mut meta = BTreeMap::new();
        meta.insert(String::from("search"), Dynamic::Map(search));

        assert!(is_search_excluded(&meta));

        meta.clear();
        assert!(!is_search_excluded(&meta));
    }
}
