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

//! Project settings.

use pyo3::FromPyObject;
use serde::Serialize;

use crate::structure::dynamic::Dynamic;
use crate::structure::nav::NavigationItem;

use super::extra::ExtraScript;
use super::mdx::MdxConfigs;
use super::plugins::Plugins;
use super::theme::Theme;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Project settings.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Project {
    /// Site name.
    pub site_name: String,
    /// Site URL.
    pub site_url: Option<String>,
    /// Site description.
    pub site_description: Option<String>,
    /// Site author.
    pub site_author: Option<String>,
    /// Docs directory (sources).
    pub docs_dir: String,
    /// Site directory (outputs).
    pub site_dir: String,
    /// Whether to use directory URLs.
    pub use_directory_urls: bool,
    /// Development server address.
    pub dev_addr: String,
    /// Copyright notice.
    pub copyright: Option<String>,
    /// Repository URL.
    pub repo_url: Option<String>,
    /// Repository name.
    pub repo_name: Option<String>,
    /// Edit URI template.
    pub edit_uri_template: Option<String>,
    /// Edit URI.
    pub edit_uri: Option<String>,
    /// Theme settings.
    pub theme: Theme,
    /// Extra settings.
    pub extra: Dynamic,
    /// Extra CSS files.
    pub extra_css: Vec<String>,
    /// Extra JavaScript files.
    pub extra_javascript: Vec<ExtraScript>,
    /// Extra template files.
    pub extra_templates: Vec<String>,
    /// Markdown extension configuration.
    pub mdx_configs: MdxConfigs,
    /// Plugins.
    pub plugins: Plugins,
    /// Navigation structure.
    pub nav: Vec<NavigationItem>,
}
