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

//! Mkdocstrings compatibility plugin.

use pyo3::types::PyAnyMethods;
use pyo3::Python;
use std::fs;
use std::path::PathBuf;

use zrx::id::Id;
use zrx::stream::Signal;

use crate::config::Config;
use crate::path::{OutputRoot, SitePath};
use crate::structure::nav::Navigation;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Mkdocstrings compatibility pipeline.
#[derive(Clone, Debug)]
pub struct Mkdocstrings {
    /// Cache directory shared with the Python compatibility layer.
    cache: PathBuf,
    /// Site output directory.
    output: OutputRoot,
}

// ----------------------------------------------------------------------------

/// Inputs required to generate the object inventory.
pub struct Dependencies<'a> {
    /// Revision-complete site navigation.
    pub navigation: &'a Signal<Id, Navigation>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Mkdocstrings {
    /// Resolves the private settings owned by this pipeline instance.
    pub fn new(config: &Config) -> Self {
        Self {
            cache: config.get_cache_dir(),
            output: config.output_root().clone(),
        }
    }

    /// Installs object inventory generation.
    pub fn setup(&self, dependencies: Dependencies<'_>) {
        let pipeline = self.clone();
        let _ = dependencies.navigation.map(move |_: &Navigation| {
            let cache_path = pipeline.cache.join("objects.inv");
            let cached = fs::read(&cache_path).ok();

            let data = Python::attach(|py| {
                let module = py.import("zensical.compat.mkdocstrings")?;
                module
                    .call_method1("get_inventory", (cached,))?
                    .extract::<Vec<u8>>()
            })?;

            let path = pipeline.output.join(
                &"objects.inv".parse::<SitePath>().expect("static site path"),
            );
            fs::create_dir_all(path.parent().expect("invariant"))?;
            fs::write(path, &data)?;
            fs::create_dir_all(&pipeline.cache)?;
            fs::write(&cache_path, &data)?;
            Ok::<_, anyhow::Error>(())
        });
    }
}
