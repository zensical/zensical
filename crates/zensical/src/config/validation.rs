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

//! Validation settings.

use pyo3::FromPyObject;
use serde::Serialize;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Validation settings.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Validation {
    /// Warn about unresolved references.
    pub unresolved_references: bool,
    /// Warn about unresolved footnotes.
    pub unresolved_footnotes: bool,
    /// Warn about unused definitions.
    pub unused_definitions: bool,
    /// Warn about unused footnotes.
    pub unused_footnotes: bool,
    /// Warn about shadowed definitions.
    pub shadowed_definitions: bool,
    /// Warn about shadowed footnotes.
    pub shadowed_footnotes: bool,
    /// Invalid links.
    pub invalid_links: bool,
    /// Invalid link anchors.
    pub invalid_link_anchors: bool,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Default for Validation {
    /// Create validation settings.
    #[inline]
    fn default() -> Self {
        Self {
            unresolved_references: true,
            unresolved_footnotes: true,
            unused_definitions: true,
            unused_footnotes: true,
            shadowed_definitions: true,
            shadowed_footnotes: true,
            invalid_links: true,
            invalid_link_anchors: true,
        }
    }
}
