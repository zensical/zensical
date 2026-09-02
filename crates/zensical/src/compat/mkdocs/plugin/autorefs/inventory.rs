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

//! Autorefs inventory.

use ahash::HashMap;
use pyo3::types::PyAnyMethods;
use pyo3::Python;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Cached global inventory URLs supplied by mkdocstrings handlers.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    /// Absolute inventory URLs.
    inventory: HashMap<String, String>,
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Collects and caches global inventory URLs supplied by mkdocstrings.
pub fn load(directory: &Path) -> HashMap<String, String> {
    let path = directory.join("autorefs.json");
    let mut cache = fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Cache>(&data).ok())
        .unwrap_or_default();

    // An absent value means all pages came from the Markdown cache and Python
    // never loaded mkdocstrings handlers. An empty map means rendering ran and
    // no external inventory is configured, so it deliberately clears cache.
    if let Some(inventory) = collect() {
        cache.inventory = inventory;
    }

    if let Ok(data) = serde_json::to_vec_pretty(&cache) {
        let _ = fs::create_dir_all(directory);
        let _ = fs::write(path, data);
    }
    cache.inventory
}

/// Collects global inventory URLs if Python rendered at least one page.
fn collect() -> Option<HashMap<String, String>> {
    Python::attach(|py| {
        let module = py.import("zensical.extensions.autorefs")?;
        module
            .call_method0("get_autorefs_inventory_data")?
            .extract::<Option<HashMap<String, String>>>()
    })
    .unwrap_or_default()
}
