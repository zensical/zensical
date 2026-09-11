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

//! mkdocs-api-autonav settings.

use crate::structure::dynamic::Dynamic;
use pyo3::types::{PyAny, PyAnyMethods, PyDict, PyDictMethods};
use pyo3::{Bound, FromPyObject, PyResult};
use serde::Serialize;
use std::collections::BTreeMap;

/// API autonav plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct ApiAutonavPlugin {
    /// Validated configuration.
    pub config: ApiAutonavConfig,
}

/// API autonav settings.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct ApiAutonavConfig {
    /// Whether the generator is enabled.
    pub enabled: bool,
    /// Absolute module or package paths.
    pub modules: Vec<String>,
    /// Ordered regex-to-option mappings; later matches override earlier ones.
    #[pyo3(from_py_with = module_options)]
    pub module_options: Vec<(String, BTreeMap<String, Dynamic>)>,
    /// Identifier prefixes and re:-prefixed expressions to exclude.
    pub exclude: Vec<String>,
    /// Navigation section title.
    pub nav_section_title: String,
    /// Documentation-relative output directory.
    pub api_root_uri: String,
    /// HTML prefix for navigation labels.
    pub nav_item_prefix: String,
    /// Whether any private namespace component excludes a module.
    pub exclude_private: bool,
    /// Whether navigation labels contain the entire identifier.
    pub show_full_namespace: bool,
    /// Policy for directories with Python files but no initializer.
    pub on_implicit_namespace_package: String,
}

fn module_options(
    value: &Bound<'_, PyAny>,
) -> PyResult<Vec<(String, BTreeMap<String, Dynamic>)>> {
    value
        .cast::<PyDict>()?
        .iter()
        .map(|(key, value)| Ok((key.extract()?, value.extract()?)))
        .collect()
}
