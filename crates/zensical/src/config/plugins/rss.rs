// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

//! Configuration for MkDocs RSS plugin compatibility.

use pyo3::FromPyObject;
use serde::Serialize;
use std::collections::BTreeMap;

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
    pub enabled: bool,
    pub abstract_chars_count: i64,
    pub abstract_delimiter: String,
    pub categories: Vec<String>,
    pub comments_path: Option<String>,
    pub date_from_meta: RssDateConfig,
    pub feed_description: Option<String>,
    pub feed_title: Option<String>,
    pub feed_ttl: u32,
    pub feeds_filenames: RssFilenames,
    pub image: Option<String>,
    pub json_feed_enabled: bool,
    pub length: usize,
    pub match_path: String,
    pub pretty_print: bool,
    pub rss_feed_enabled: bool,
    pub stylesheet: String,
    pub url_parameters: BTreeMap<String, String>,
    pub use_git: bool,
    pub use_material_blog: bool,
    pub use_material_social_cards: bool,
}

/// Metadata date keys and their interpretation.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RssDateConfig {
    pub as_creation: String,
    pub as_update: String,
    pub datetime_format: String,
    pub default_time: String,
    pub default_timezone: String,
}

/// Site-relative output names for the four feed variants.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct RssFilenames {
    pub json_created: String,
    pub json_updated: String,
    pub rss_created: String,
    pub rss_updated: String,
}
