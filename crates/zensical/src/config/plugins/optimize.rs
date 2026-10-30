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

//! Material optimize plugin configuration.

use pyo3::FromPyObject;
use serde::Serialize;

/// Ordered optimize plugin instances.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct OptimizePlugin {
    /// Instances normalized by the Python configuration bridge.
    pub config: Vec<OptimizePluginInstance>,
}

/// One configured optimize plugin instance.
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct OptimizePluginInstance {
    /// Configured instance name.
    pub name: String,
    /// Validated options.
    pub config: OptimizePluginConfig,
}

/// Material-compatible optimize options.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct OptimizePluginConfig {
    /// Whether this instance is enabled.
    pub enabled: bool,
    /// Maximum number of assets processed concurrently.
    pub concurrency: usize,
    /// Whether optimized bytes are reused between builds.
    pub cache: bool,
    /// Project-relative or absolute optimization cache directory.
    pub cache_dir: String,
    /// Whether image optimization is enabled.
    pub optimize: bool,
    /// Whether PNG images are optimized.
    pub optimize_png: bool,
    /// PNG speed and quality tradeoff from 1 to 10.
    pub optimize_png_speed: u8,
    /// Whether optional PNG metadata is removed.
    pub optimize_png_strip: bool,
    /// Whether JPEG images are optimized.
    pub optimize_jpg: bool,
    /// JPEG quality from 0 to 100.
    pub optimize_jpg_quality: u8,
    /// Whether progressive JPEG encoding is used.
    pub optimize_jpg_progressive: bool,
    /// Inclusion patterns, taking precedence over exclusion patterns.
    pub optimize_include: Vec<String>,
    /// Exclusion patterns used when no inclusions are configured.
    pub optimize_exclude: Vec<String>,
    /// Whether savings are reported for each image.
    pub print_gain: bool,
    /// Whether total savings are reported after the build.
    pub print_gain_summary: bool,
}
