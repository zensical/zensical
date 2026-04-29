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

//! Anchor.

use pyo3::prelude::*;
use std::slice::Iter;
use std::str::FromStr;
use zrx::stream::Value;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Anchor set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchors {
    /// Inner set of anchors.
    inner: Vec<String>,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Anchors {}

// ----------------------------------------------------------------------------

impl FromStr for Anchors {
    type Err = PyErr;

    /// Parses anchors from HTML.
    #[inline]
    fn from_str(html: &str) -> PyResult<Self> {
        Python::attach(|py| {
            let module = py.import("zensical.collectors")?;

            // The anchors method returns an iterator of anchors
            let iter = module
                .call_method1("anchors", (html,))?
                .try_iter()?
                .map(|item| item?.extract::<String>());

            // Collect anchors into an anchor set
            Ok(Anchors {
                inner: iter.collect::<PyResult<_>>()?,
            })
        })
    }
}

// ----------------------------------------------------------------------------

impl<'a> IntoIterator for &'a Anchors {
    type Item = &'a String;
    type IntoIter = Iter<'a, String>;

    /// Creates an iterator over the anchors.
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}
