// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

//! Configuration for localized Git revision dates.

use pyo3::FromPyObject;
use serde::Serialize;

/// Git revision date plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct GitRevisionDateLocalizedPlugin {
    /// Plugin configuration.
    pub config: GitRevisionDateLocalizedPluginConfig,
}

/// History selection and localized output settings.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct GitRevisionDateLocalizedPluginConfig {
    /// Whether Git dates are inserted.
    pub enabled: bool,
    /// Selected date representation.
    pub r#type: String,
    /// Explicit locale, before the theme language fallback.
    pub locale: Option<String>,
    /// IANA time zone used for display.
    pub timezone: String,
    /// Localized strftime pattern used by custom dates.
    pub custom_format: String,
    /// Whether Git failures are replaced with the build timestamp.
    pub fallback_to_build_date: bool,
    /// Whether the original creation commit is queried.
    pub enable_creation_date: bool,
    /// Whether history is followed across renames.
    pub enable_git_follow: bool,
    /// Whether page queries are run concurrently.
    pub enable_parallel_processing: bool,
    /// Whether fallback diagnostics are emitted as warnings.
    pub strict: bool,
    /// Documentation-relative exclusion patterns.
    pub exclude: Vec<String>,
    /// Configuration-relative list of ignored revision commits.
    pub ignored_commits_file: Option<String>,
}
