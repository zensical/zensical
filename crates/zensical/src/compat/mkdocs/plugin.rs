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
use crate::structure::markdown::Markdown;

use super::html::{self, Visitor};

pub mod autorefs;
pub mod meta;
pub mod minify;
pub mod mkdocstrings;
pub mod redirects;
pub mod search;

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
}

// ----------------------------------------------------------------------------

/// Enabled MkDocs-compatible participants in the shared Markdown HTML pass.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Whether autorefs extraction and settlement are active.
    pub autorefs: bool,
    /// Whether search extraction is active.
    search: bool,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Settings {
    /// Derives active compatibility participants from resolved configuration.
    pub fn new(config: &Config) -> Self {
        Self {
            autorefs: autorefs::is_enabled(config),
            search: config.project.plugins.search.config.enabled,
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Runs enabled MkDocs-compatible visitors in one page-local HTML pass.
pub fn prepare(markdown: &mut Markdown, settings: Settings) -> HtmlFacts {
    let mut autorefs = autorefs::Parser::default();
    let mut search = search::parser(&markdown.meta);

    let content = match (settings.search, settings.autorefs) {
        (true, true) => {
            let mut visitors: [&mut dyn Visitor; 2] =
                [&mut search, &mut autorefs];
            html::scan(&markdown.content, &mut visitors)
        }
        (true, false) => html::scan(&markdown.content, &mut [&mut search]),
        (false, true) => html::scan(&markdown.content, &mut [&mut autorefs]),
        (false, false) => None,
    };
    if let Some(content) = content {
        markdown.replace_content(content);
    }

    HtmlFacts {
        autorefs: if settings.autorefs {
            Arc::new(autorefs.finish())
        } else {
            Arc::default()
        },
        search: if settings.search {
            search::finish(search)
        } else {
            Arc::default()
        },
    }
}
