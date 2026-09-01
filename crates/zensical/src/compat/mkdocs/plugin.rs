// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! MkDocs-compatible plugins.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::html::{self, Visitor};
use crate::config::Config;
use crate::structure::markdown::Markdown;

pub mod autorefs;
pub mod mkdocstrings;
pub mod search;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Cached facts produced by the shared Markdown HTML pass.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct HtmlFacts {
    /// Page-local autoref placeholders replaced with stable slots.
    pub autorefs: Arc<autorefs::References>,
    /// Page-local search sections.
    pub search: Arc<search::Facts>,
}

// ----------------------------------------------------------------------------

/// Enabled MkDocs-compatible participants in the shared Markdown HTML pass.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
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
    pub(crate) fn new(config: &Config) -> Self {
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
pub(crate) fn prepare(
    markdown: &mut Markdown, settings: Settings,
) -> HtmlFacts {
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
