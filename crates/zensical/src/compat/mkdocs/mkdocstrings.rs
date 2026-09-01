// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Mkdocstrings compatibility artifacts.

use pyo3::types::PyAnyMethods;
use pyo3::Python;
use std::fs;
use zrx::id::Id;
use zrx::stream::Signal;

use crate::config::Config;
use crate::structure::nav::Navigation;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Attach object inventory generation to the settled navigation stream.
pub(crate) fn attach(config: &Config, nav: &Signal<Id, Navigation>) {
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
            let site_dir = config.get_site_dir();
            let path = site_dir.join("objects.inv");
            let _ = fs::create_dir_all(path.parent().expect("invariant"));
            let _ = fs::write(path, &data);
            let _ = fs::create_dir_all(&cache_dir);
            let _ = fs::write(&cache_path, &data);
        }
    });
}
