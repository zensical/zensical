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

//! Tag.

use pyo3::types::{PyAny, PyAnyMethods};
use pyo3::{Bound, FromPyObject, PyResult};
use serde::Serialize;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Tag.
#[derive(Clone, Debug, Hash, PartialEq, Eq, FromPyObject, Serialize)]
pub struct Tag {
    /// Tag name.
    pub name: String,
    /// Parent tag, if this tag belongs to a hierarchy.
    pub parent: Option<TagNode>,
    /// Primary listing URL, if any.
    pub url: Option<String>,
    /// Whether presentation classifies the tag as hidden.
    pub hidden: bool,
    /// Every matching listing URL in preference order.
    pub links: Vec<TagLink>,
}

/// Template-visible tag node without listing references.
#[derive(Clone, Debug, Hash, PartialEq, Eq, FromPyObject, Serialize)]
pub struct TagNode {
    /// Cumulative tag name.
    pub name: String,
    /// Parent tag, if this tag belongs to a hierarchy.
    #[pyo3(default, from_py_with = extract_parent)]
    pub parent: Option<Box<TagNode>>,
    /// Whether presentation classifies the tag as hidden.
    pub hidden: bool,
}

/// Link from a page tag to one listing.
#[derive(Clone, Debug, Hash, PartialEq, Eq, FromPyObject, Serialize)]
pub struct TagLink {
    /// Listing page title.
    pub title: String,
    /// Listing tag URL.
    pub url: String,
}

// ----------------------------------------------------------------------------

/// Extracts an optional recursive tag parent from a Python tag object.
fn extract_parent(value: &Bound<'_, PyAny>) -> PyResult<Option<Box<TagNode>>> {
    if value.is_none() {
        Ok(None)
    } else {
        value.extract().map(Box::new).map(Some)
    }
}
