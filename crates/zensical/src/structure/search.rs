// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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

//! Search index.

use pyo3::FromPyObject;
use serde::Serialize;
use zrx::id::Id;
use zrx::scheduler::Value;
use zrx::stream::value::Chunk;

use crate::config::plugins::SearchPluginConfig;

use super::nav::{file_sort_key, Navigation};
use super::page::Page;

mod item;

pub use item::SearchItem;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Search configuration.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject, Serialize)]
pub struct SearchConfig {
    /// Separator for tokenizer.
    pub separator: String,
}

/// Search index.
///
/// Later, when the module system is available, we'll move search into a module
/// of its own, but for now, we'll just keep it here for simplicity.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject, Serialize)]
pub struct SearchIndex {
    /// Search configuration.
    pub config: SearchConfig,
    /// Search items.
    pub items: Vec<SearchItem>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl SearchIndex {
    /// Creates a search index from pages.
    #[allow(clippy::assigning_clones)]
    pub fn new(
        pages: Chunk<Id, Page>, nav: &Navigation, config: SearchPluginConfig,
    ) -> Self {
        let mut items: Vec<SearchItem> = Vec::new();

        // Convert chunk into a vector for easier processing, and sort pages by
        // the exact same method that MkDocs uses
        let mut pages = Vec::from_iter(pages);
        pages.sort_by_key(|item| file_sort_key(&item.id));

        // Assemble search index, combining all items from all pages into a
        // single, flat list, adjusting the location to include the page URL
        for page in pages {
            let iter = nav.ancestors(&page.data).into_iter().rev();
            let mut path = iter
                .map(|item| item.title.expect("invariant"))
                .collect::<Vec<_>>();

            // Add page title to path if not already present - this might be
            // the true in case of index pages
            if path.last() != Some(&page.data.title) {
                path.push(page.data.title.clone());
            }

            // Extract page tags, if any
            let tags: Vec<String> =
                page.data.tags().into_iter().map(|tag| tag.name).collect();

            // For each page, adjust the location of each item and add it to
            // the overall list
            for mut item in page.data.search {
                let location = match item.location {
                    Some(id) => format!("{}#{}", page.data.url, id),
                    _ => page.data.url.clone(),
                };

                // Fall back to page title, if item title is empty
                if item.title.is_empty() {
                    item.title = page.data.title.clone();
                }

                // Update location and path and add item
                item.location = Some(location);
                item.path = path.clone();
                item.tags = tags.clone();
                items.push(item);
            }
        }

        // Return search
        Self { config: config.into(), items }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for SearchIndex {}

// ----------------------------------------------------------------------------

impl From<SearchPluginConfig> for SearchConfig {
    /// Converts plugin configuration into search configuration.
    fn from(config: SearchPluginConfig) -> Self {
        Self { separator: config.separator }
    }
}
