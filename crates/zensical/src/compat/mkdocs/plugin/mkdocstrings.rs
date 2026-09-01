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

use zrx::id::Id;
use zrx::stream::Signal;

use crate::config::Config;
use crate::path::SitePath;
use crate::structure::nav::Navigation;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Attach object inventory generation to the settled navigation stream.
pub fn attach(config: &Config, nav: &Signal<Id, Navigation>) {
    let config = config.clone();
    let _ = nav.map(move |_: &Navigation| {
        let cache_dir = config.get_cache_dir();
        let cache_path = cache_dir.join("objects.inv");
        let cached = fs::read(&cache_path).ok();

        let data = Python::attach(|py| {
            let module = py.import("zensical.compat.mkdocstrings")?;
            module
                .call_method1("get_inventory", (cached,))?
                .extract::<Vec<u8>>()
        });

        if let Ok(data) = data {
            let path = config.output_root().join(
                &"objects.inv".parse::<SitePath>().expect("static site path"),
            );
            let _ = fs::create_dir_all(path.parent().expect("invariant"));
            let _ = fs::write(path, &data);
            let _ = fs::create_dir_all(&cache_dir);
            let _ = fs::write(&cache_path, &data);
        }
    });
}
