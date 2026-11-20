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
