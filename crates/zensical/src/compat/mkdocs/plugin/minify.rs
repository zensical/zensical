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

//! MkDocs-compatible minify plugin.

use std::path::Path;

use crate::config::plugins::MinifyPluginConfig;
use crate::config::Config;

pub mod asset;
mod html;
mod script;
mod style;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Resolved minification settings shared by page render tasks.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Normalized MkDocs-compatible minification configuration.
    config: MinifyPluginConfig,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Settings {
    /// Resolves minification settings from project configuration.
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.project.plugins.minify.config.clone(),
        }
    }

    /// Returns the normalized plugin configuration for the asset stage.
    pub fn config(&self) -> &MinifyPluginConfig {
        &self.config
    }

    /// Processes one final HTML document after all compatibility mutations.
    ///
    /// External asset options stay in the resolved plugin configuration for
    /// the asset output stage and do not affect this document-local pass.
    pub fn html(&self, content: impl Into<String>) -> String {
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
    pub fn template(&self, name: &str, content: impl Into<String>) -> String {
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
