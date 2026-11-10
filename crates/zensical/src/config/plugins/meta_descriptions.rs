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

//! Configuration for automatic meta descriptions.

use pyo3::FromPyObject;
use serde::Serialize;

/// Meta descriptions plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct MetaDescriptionsPlugin {
    /// Plugin configuration.
    pub config: MetaDescriptionsPluginConfig,
}

/// Paragraph extraction, validation, and export settings.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct MetaDescriptionsPluginConfig {
    /// Whether descriptions are generated.
    pub enabled: bool,
    /// Whether rendered descriptions are exported to CSV.
    pub export_csv: bool,
    /// Whether informational messages are suppressed.
    pub quiet: bool,
    /// Whether missing descriptions and length limits are checked.
    pub enable_checks: bool,
    /// Minimum description length in Unicode characters.
    pub min_length: usize,
    /// Maximum description length in Unicode characters.
    pub max_length: usize,
    /// Whether generated descriptions are shortened with textwrap semantics.
    pub trim: bool,
    /// Whether short paragraphs are ignored in favor of the site description.
    pub fallback_if_short: bool,
}
