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

//! Configuration for MkDocs RSS plugin compatibility.

use pyo3::FromPyObject;
use serde::Serialize;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Ordered RSS plugin instances.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RssPlugin {
    /// Normalized instances.
    pub config: Vec<RssPluginInstance>,
}

/// One configured RSS plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RssPluginInstance {
    /// Instance name.
    pub name: String,
    /// Instance settings.
    pub config: RssPluginConfig,
}

/// RSS output and page-selection settings.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RssPluginConfig {
    /// Enables this plugin instance.
    pub enabled: bool,
    /// Character limit for inferred abstracts, or `-1` for full content.
    pub abstract_chars_count: i64,
    /// Optional Markdown excerpt marker.
    pub abstract_delimiter: String,
    /// Metadata fields used as item categories.
    pub categories: Vec<String>,
    /// Suffix appended to canonical URLs for comment links.
    pub comments_path: Option<String>,
    /// Metadata date keys, parser format, and fallback timezone.
    pub date_from_meta: RssDateConfig,
    /// Override for the channel description.
    pub feed_description: Option<String>,
    /// Override for the channel title.
    pub feed_title: Option<String>,
    /// RSS time to live in minutes.
    pub feed_ttl: u32,
    /// Output paths for the four feed variants.
    pub feeds_filenames: RssFilenames,
    /// Channel image or JSON Feed icon URL.
    pub image: Option<String>,
    /// Enables JSON Feed output.
    pub json_feed_enabled: bool,
    /// Maximum item count in each feed variant.
    pub length: usize,
    /// Source path selection pattern.
    pub match_path: String,
    /// Enables formatted feed output.
    pub pretty_print: bool,
    /// Enables RSS 2.0 output.
    pub rss_feed_enabled: bool,
    /// Feed stylesheet URL, or `auto` for the bundled stylesheet.
    pub stylesheet: String,
    /// Query pairs in user configuration order.
    pub url_parameters: Vec<(String, String)>,
    /// Enables Git dates as a metadata fallback.
    pub use_git: bool,
    /// Enables Material blog author metadata.
    pub use_material_blog: bool,
}

// ----------------------------------------------------------------------------

/// Metadata date keys and their interpretation.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RssDateConfig {
    /// Metadata key for the creation date.
    pub as_creation: String,
    /// Metadata key for the update date.
    pub as_update: String,
    /// Custom datetime parser format.
    pub datetime_format: String,
    /// Time used for date-only metadata.
    pub default_time: String,
    /// Timezone used for naive metadata and Git dates.
    pub default_timezone: String,
}

// ----------------------------------------------------------------------------

/// Site-relative output names for the four feed variants.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RssFilenames {
    /// JSON Feed ordered by creation date.
    pub json_created: String,
    /// JSON Feed ordered by update date.
    pub json_updated: String,
    /// RSS feed ordered by creation date.
    pub rss_created: String,
    /// RSS feed ordered by update date.
    pub rss_updated: String,
}
