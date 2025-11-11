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

//! Navigation item.

use pyo3::FromPyObject;
use serde::Serialize;

use crate::structure::page::PageMeta;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Navigation item metadata.
#[derive(Clone, Debug, PartialEq, Hash, Eq, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct NavigationMeta {
    /// Page icon.
    pub icon: Option<String>,
    /// Page status.
    pub status: Option<String>,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl From<PageMeta> for NavigationMeta {
    /// Extract navigation metadata from a page.
    fn from(meta: PageMeta) -> Self {
        let icon = meta.get("icon").cloned();
        let status = meta.get("status").cloned();
        NavigationMeta {
            icon: icon.map(|meta| meta.to_string()),
            status: status.map(|meta| meta.to_string()),
        }
    }
}
