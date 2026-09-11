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

//! mkdocs-autoapi settings.

use crate::structure::dynamic::Dynamic;
use pyo3::FromPyObject;
use serde::Serialize;

/// AutoAPI plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct AutoApiPlugin {
    /// Validated configuration.
    pub config: AutoApiConfig,
}

/// AutoAPI settings.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct AutoApiConfig {
    /// Whether the generator is enabled.
    pub enabled: bool,
    /// Source directory, resolved against the project root.
    pub autoapi_dir: String,
    /// Recursive inclusion patterns, in priority order.
    pub autoapi_file_patterns: Vec<String>,
    /// Root-relative exclusion patterns.
    pub autoapi_ignore: Vec<String>,
    /// Whether generated Markdown is also saved in docs_dir.
    pub autoapi_keep_files: bool,
    /// Whether to generate pages, or only link existing documentation.
    pub autoapi_generate_api_docs: bool,
    /// Boolean or section title controlling automatic navigation insertion.
    pub autoapi_add_nav_entry: Dynamic,
    /// Documentation-relative output directory.
    pub autoapi_root: String,
    /// Configured mkdocstrings default handler.
    pub handler: String,
}
