// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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

//! Plugin settings.

use pyo3::FromPyObject;
use serde::Serialize;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Plugin settings.
///
/// This data type includes configuration for functionality that is implemented
/// as part of plugins in MkDocs. Right now, this is only a small subset, and
/// only provided for compatibility with our templates. We'll replace this with
/// the module system in the near future.
///
/// Also note that we require the plugins to be set, which is ensured by the
/// configuration parser that is currently implemented in Python.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Plugins {
    /// Search plugin.
    pub search: SearchPlugin,
    /// Offline plugin.
    pub offline: OfflinePlugin,
}

// ----------------------------------------------------------------------------

/// Search plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct SearchPlugin {
    /// Plugin configuration.
    pub config: SearchPluginConfig,
}

/// Search plugin configuration.
///
/// This second layer is necessary to make our templates compatible with
/// Material for MkDocs, since MkDocs exposes the search plugin instance.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct SearchPluginConfig {
    /// Whether the search plugin is enabled.
    pub enabled: bool,
    /// Tokenizer separator.
    pub separator: String,
}

// ----------------------------------------------------------------------------

/// Offline plugin.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct OfflinePlugin {
    /// Plugin configuration.
    pub config: OfflinePluginConfig,
}

/// Offline plugin configuration.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct OfflinePluginConfig {
    /// Whether the offline plugin is enabled.
    pub enabled: bool,
}
