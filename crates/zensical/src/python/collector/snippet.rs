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

//! Snippet.

use pyo3::prelude::*;
use std::slice::Iter;
use std::str::FromStr;
use zrx::stream::Value;

mod file;
mod range;

pub use file::SnippetFile;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Snippet.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject)]
pub struct Snippet {
    /// Start offset.
    pub start: usize,
    /// End offset.
    pub end: usize,
    /// Indent level.
    pub indent: usize,
    /// File references.
    pub files: Vec<SnippetFile>,
}

// ----------------------------------------------------------------------------

/// Snippet set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippets {
    /// Inner set of snippets.
    inner: Vec<Snippet>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Value for Snippets {}

// ----------------------------------------------------------------------------

impl FromStr for Snippets {
    type Err = PyErr;

    /// Parses snippets from Markdown.
    #[inline]
    fn from_str(markdown: &str) -> PyResult<Self> {
        Python::attach(|py| {
            let module = py.import("zensical.collectors")?;

            // The snippets method returns an iterator of snippets
            let iter = module
                .call_method1("snippets", (markdown.as_bytes(),))?
                .try_iter()?
                .map(|item| item?.extract::<Snippet>());

            // Collect snippets into a snippet set
            Ok(Snippets {
                inner: iter.collect::<PyResult<_>>()?,
            })
        })
    }
}

// ----------------------------------------------------------------------------

impl<'a> IntoIterator for &'a Snippets {
    type Item = &'a Snippet;
    type IntoIter = Iter<'a, Snippet>;

    /// Creates an iterator over the snippets.
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}
