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

//! Configuration for LLM text output.

use pyo3::FromPyObject;
use serde::Serialize;

/// MkDocs LLM text plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct LlmstxtPlugin {
    /// Plugin configuration.
    pub config: LlmstxtPluginConfig,
}

/// Ordered page selection and output settings.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct LlmstxtPluginConfig {
    /// Whether to emit LLM text files.
    pub enabled: bool,
    /// Whether to remove decorative HTML and code line numbers.
    pub autoclean: bool,
    /// Public URL overriding the site's canonical URL.
    pub base_url: Option<String>,
    /// Additional Markdown after the site description.
    pub markdown_description: Option<String>,
    /// Optional site-relative file containing all selected pages.
    pub full_output: Option<String>,
    /// Sections in configuration order.
    pub sections: Vec<Section>,
}

/// One named section of the index.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Section {
    /// Heading in the generated index.
    pub title: String,
    /// Page selectors in configuration order.
    pub inputs: Vec<Input>,
}

/// One page path or Python fnmatch pattern.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Input {
    /// Original source path or pattern.
    pub pattern: String,
    /// Optional description appended to the index link.
    pub description: String,
    /// Regular expression for a wildcard selector.
    pub expression: Option<String>,
}
