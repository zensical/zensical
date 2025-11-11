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

//! Theme settings.

use pyo3::FromPyObject;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Theme settings.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Theme {
    /// Theme custom directory.
    pub custom_dir: Option<PathBuf>,
    /// Theme variant.
    pub variant: Option<String>,
    /// Language.
    pub language: String,
    /// Text direction.
    pub direction: Option<String>,
    /// Feature flags.
    pub features: Vec<String>,
    /// Font settings.
    pub font: Font,
    /// Static templates.
    pub static_templates: Vec<String>,
    /// Favicon.
    pub favicon: Option<String>,
    /// Logo.
    pub logo: Option<String>,
    /// Icon settings.
    pub icon: Icon,
    /// Color palette settings.
    pub palette: Vec<Palette>,
}

// ----------------------------------------------------------------------------

/// Font settings.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[serde(untagged)]
#[pyo3(from_item_all)]
pub enum Font {
    /// Use custom fonts.
    Custom(CustomFont),
    /// Use system fonts.
    System(bool),
}

/// Custom fonts.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct CustomFont {
    /// Text font.
    pub text: String,
    /// Code font.
    pub code: String,
}

// ----------------------------------------------------------------------------

/// Icon settings.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Icon {
    /// Edit button icon.
    pub edit: Option<String>,
    /// View button icon.
    pub view: Option<String>,
    /// Logo icon.
    pub logo: Option<String>,
    /// Repository icon.
    pub repo: Option<String>,
    /// Annotation icon.
    pub annotation: Option<String>,
    /// Back-to-top icon.
    pub top: Option<String>,
    /// Search sharing icon.
    pub share: Option<String>,
    /// Menu icon.
    pub menu: Option<String>,
    /// Alternate languages icon.
    pub alternate: Option<String>,
    /// Search icon.
    pub search: Option<String>,
    /// Close icon.
    pub close: Option<String>,
    /// Previous page icon.
    pub previous: Option<String>,
    /// Next page icon.
    pub next: Option<String>,
    /// Admonition icons.
    pub admonition: Option<AdmonitionIcon>,
    /// Tag icons.
    pub tag: BTreeMap<String, String>,
}

/// Admonition icons.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct AdmonitionIcon {
    /// Admonition `note` icon.
    pub note: Option<String>,
    /// Admonition `abstract` icon.
    pub r#abstract: Option<String>,
    /// Admonition `info` icon.
    pub info: Option<String>,
    /// Admonition `tip` icon.
    pub tip: Option<String>,
    /// Admonition `success` icon.
    pub success: Option<String>,
    /// Admonition `question` icon.
    pub question: Option<String>,
    /// Admonition `warning` icon.
    pub warning: Option<String>,
    /// Admonition `failure` icon.
    pub failure: Option<String>,
    /// Admonition `danger` icon.
    pub danger: Option<String>,
    /// Admonition `bug` icon.
    pub bug: Option<String>,
    /// Admonition `example` icon.
    pub example: Option<String>,
    /// Admonition `quote` icon.
    pub quote: Option<String>,
}

// ----------------------------------------------------------------------------

/// Color palette settings.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Palette {
    /// Palette media query.
    pub media: Option<String>,
    /// Palette scheme.
    pub scheme: Option<String>,
    /// Palette primary color.
    pub primary: Option<String>,
    /// Palette accent color.
    pub accent: Option<String>,
    /// Palette toggle.
    pub toggle: Option<PaletteToggle>,
}

/// Color palette toggle.
#[derive(Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct PaletteToggle {
    /// Palette toggle icon.
    pub icon: Option<String>,
    /// Palette toggle name.
    pub name: Option<String>,
}
