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

//! MkDocs-compatible plugins.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::markdown::Markdown;

use super::html::{self, Visitor};

pub mod autorefs;
pub mod awesome_nav;
pub mod literate_nav;
pub mod meta;
pub mod minify;
pub mod mkdocstrings;
pub mod redirects;
pub mod search;
pub mod tags;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Cached facts produced by the shared Markdown HTML pass.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct HtmlFacts {
    /// Page-local autoref placeholders replaced with stable slots.
    pub autorefs: Arc<autorefs::References>,
    /// Page-local search sections.
    pub search: Arc<search::Facts>,
    /// Page-local tag mappings and listing slots.
    pub tags: Arc<tags::Facts>,
}

// ----------------------------------------------------------------------------

/// Enabled MkDocs-compatible participants in the shared Markdown HTML pass.
#[derive(Clone, Debug)]
pub struct Settings {
    /// MkDocs-compatible autorefs pipeline.
    pub autorefs: Arc<autorefs::Autorefs>,
    /// MkDocs-compatible search pipeline.
    pub search: Arc<search::Search>,
    /// Material tags compatibility pipeline.
    pub tags: tags::Tags,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Settings {
    /// Derives active compatibility participants from resolved configuration.
    pub fn new(config: &Config, serve: bool) -> Self {
        Self {
            autorefs: Arc::new(autorefs::Autorefs::new(config)),
            search: Arc::new(search::Search::new(config)),
            tags: tags::Tags::new(config, serve),
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Runs enabled MkDocs-compatible visitors in one page-local HTML pass.
pub fn prepare(
    markdown: &mut Markdown, source: &SourcePath, settings: &Settings,
) -> anyhow::Result<HtmlFacts> {
    let mut autorefs = autorefs::Parser::default();
    let defer_search_cleanup =
        settings.search.is_enabled() && !settings.tags.is_empty();
    let mut search = search::parser(&markdown.meta);
    if defer_search_cleanup {
        search = search.retaining_directives();
    }
    let mut tags = if settings.tags.is_empty() {
        None
    } else {
        Some(tags::Parser::new(&settings.tags, source, &markdown.meta)?)
    };

    // Compose enabled observers dynamically so adding a compatibility module
    // doesn't grow an exhaustive Boolean match or another HTML traversal.
    let mut visitors = Vec::<&mut dyn Visitor>::new();
    if settings.search.is_enabled() {
        visitors.push(&mut search);
    }
    if settings.autorefs.is_enabled() {
        visitors.push(&mut autorefs);
    }
    if let Some(parser) = &mut tags {
        visitors.push(parser);
    }
    let content = (!visitors.is_empty())
        .then(|| html::scan(&markdown.content, &mut visitors))
        .flatten();
    if let Some(content) = content {
        markdown.replace_content(content);
    }

    let requires_search_cleanup = search.requires_cleanup();
    let mut tag_facts = match tags {
        Some(parser) => parser.finish()?,
        None => tags::Facts::default(),
    };
    if requires_search_cleanup {
        tag_facts.require_search_cleanup();
    }

    Ok(HtmlFacts {
        autorefs: if settings.autorefs.is_enabled() {
            Arc::new(autorefs.finish())
        } else {
            Arc::default()
        },
        search: if settings.search.is_enabled() {
            search::finish(search)
        } else {
            Arc::default()
        },
        tags: Arc::new(tag_facts),
    })
}
