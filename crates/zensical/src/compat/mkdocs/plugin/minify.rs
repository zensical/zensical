// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! MkDocs-compatible minify plugin.

use crate::config::plugins::MinifyPluginConfig;
use crate::config::Config;
use std::path::Path;

pub(crate) mod asset;
mod html;
mod script;
mod style;

/// Resolved minification settings shared by page render tasks.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    config: MinifyPluginConfig,
}

impl Settings {
    /// Resolves minification settings from project configuration.
    pub(crate) fn new(config: &Config) -> Self {
        Self {
            config: config.project.plugins.minify.config.clone(),
        }
    }

    /// Processes one final HTML document after all compatibility mutations.
    ///
    /// External asset options stay in the resolved plugin configuration for
    /// the asset output stage and do not affect this document-local pass.
    pub(crate) fn html(&self, content: impl Into<String>) -> String {
        let content = content.into();
        if !self.config.enabled {
            return content;
        }
        if self.config.minify_html {
            html::minify(
                &content,
                &self.config.htmlmin_opts,
                self.config.minify_inline_js,
                self.config.minify_inline_css,
            )
        } else if self.config.minify_inline_js || self.config.minify_inline_css
        {
            html::minify_inline(
                content,
                self.config.minify_inline_js,
                self.config.minify_inline_css,
            )
        } else {
            content
        }
    }

    /// Processes a rendered static template when it produces HTML.
    pub(crate) fn template(
        &self, name: &str, content: impl Into<String>,
    ) -> String {
        let content = content.into();
        if Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
        {
            self.html(content)
        } else {
            content
        }
    }
}
