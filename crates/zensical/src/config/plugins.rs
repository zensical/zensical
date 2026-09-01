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

//! Plugin settings.

use pyo3::FromPyObject;
use serde::Serialize;
use std::collections::BTreeMap;

mod tags;

pub use tags::{
    python_bool, python_float, python_scalar, TagsListingConfig, TagsPlugin,
    TagsPluginConfig,
};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Plugin settings.
///
/// This data type includes configuration for functionality that is implemented
/// as part of plugins in MkDocs. Right now, this is only a small subset, and
/// only provided for compatibility with our templates. We'll replace this with
/// the module system in the near future.
///
/// Also note that we require the plugins to be set, which is ensured by the
/// configuration parser that is currently implemented in Python.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Plugins {
    /// Search plugin.
    pub search: SearchPlugin,
    /// Material meta plugin.
    pub meta: MetaPlugin,
    /// Redirects plugin.
    pub redirects: RedirectsPlugin,
    /// Minify plugin.
    pub minify: MinifyPlugin,
    /// Material tags plugin instances.
    pub tags: TagsPlugin,
    /// Offline plugin.
    pub offline: OfflinePlugin,
}

// ----------------------------------------------------------------------------

/// Material meta plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct MetaPlugin {
    /// Plugin configuration.
    pub config: MetaPluginConfig,
}

/// Material meta plugin configuration.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct MetaPluginConfig {
    /// Whether metadata inheritance is enabled.
    pub enabled: bool,
    /// Name of metadata files inside the documentation tree.
    pub meta_file: String,
}

// ----------------------------------------------------------------------------

/// Redirects plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RedirectsPlugin {
    /// Plugin configuration.
    pub config: RedirectsPluginConfig,
}

/// Redirects plugin configuration.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RedirectsPluginConfig {
    /// Whether redirects are enabled.
    pub enabled: bool,
    /// Source-to-target redirect mappings.
    pub redirect_maps: BTreeMap<String, String>,
}

// ----------------------------------------------------------------------------

/// Minify plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct MinifyPlugin {
    /// Plugin configuration.
    pub config: MinifyPluginConfig,
}

/// Minify plugin configuration.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct MinifyPluginConfig {
    /// Whether the plugin is enabled.
    pub enabled: bool,
    /// Whether rendered HTML is minified.
    pub minify_html: bool,
    /// Whether selected JavaScript assets are minified.
    pub minify_js: bool,
    /// Whether selected CSS assets are minified.
    pub minify_css: bool,
    /// Whether inline JavaScript is minified.
    pub minify_inline_js: bool,
    /// Whether inline CSS is minified.
    pub minify_inline_css: bool,
    /// JavaScript asset paths or patterns.
    pub js_files: Vec<String>,
    /// CSS asset paths or patterns.
    pub css_files: Vec<String>,
    /// HTML minification options.
    pub htmlmin_opts: HtmlMinOptions,
    /// Whether asset names include a hash of their emitted contents.
    pub cache_safe: bool,
}

/// HTML options accepted by mkdocs-minify-plugin.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct HtmlMinOptions {
    /// Whether ordinary HTML comments are removed.
    pub remove_comments: bool,
    /// Whether newline-containing whitespace-only text is removed.
    pub remove_empty_space: bool,
    /// Whether all whitespace-only text is removed.
    pub remove_all_empty_space: bool,
    /// Whether empty attribute values are collapsed.
    pub reduce_empty_attributes: bool,
    /// Whether HTML boolean attribute values are collapsed.
    pub reduce_boolean_attributes: bool,
    /// Whether optional attribute quotes are removed.
    pub remove_optional_attribute_quotes: bool,
    /// Whether character references in attributes are decoded when safe.
    pub convert_charrefs: bool,
    /// Whether the preservation marker attribute remains in output.
    pub keep_pre: bool,
    /// Elements whose contents are preserved verbatim.
    pub pre_tags: Vec<String>,
    /// Attribute marking an element or attribute value for preservation.
    pub pre_attr: String,
}

/// Search plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct SearchPlugin {
    /// Plugin configuration.
    pub config: SearchPluginConfig,
}

/// Search plugin configuration.
///
/// This second layer is necessary to make our templates compatible with
/// Material for MkDocs, since MkDocs exposes the search plugin instance.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct SearchPluginConfig {
    /// Whether the search plugin is enabled.
    pub enabled: bool,
    /// Tokenizer separator.
    pub separator: String,
}

// ----------------------------------------------------------------------------

/// Offline plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct OfflinePlugin {
    /// Plugin configuration.
    pub config: OfflinePluginConfig,
}

/// Offline plugin configuration.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct OfflinePluginConfig {
    /// Whether the offline plugin is enabled.
    pub enabled: bool,
}
