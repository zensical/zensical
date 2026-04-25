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

//! Reference.

use pyo3::prelude::*;
use std::str::FromStr;

mod footnote;
mod link;

pub use footnote::{FootnoteDefinition, FootnoteReference};
pub use link::{Link, LinkDefinition, LinkReference};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Reference.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject)]
pub enum Reference {
    /// Link or image.
    Link(Link),
    /// Link reference.
    LinkReference(LinkReference),
    /// Link definition.
    LinkDefinition(LinkDefinition),
    /// Footnote reference.
    FootnoteReference(FootnoteReference),
    /// Footnote definition.
    FootnoteDefinition(FootnoteDefinition),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Reference set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct References {
    /// Inner set of references.
    inner: Vec<Reference>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl FromStr for References {
    type Err = PyErr;

    /// Parses references from Markdown.
    #[inline]
    fn from_str(markdown: &str) -> PyResult<Self> {
        Python::attach(|py| {
            let module = py.import("zensical.collectors")?;

            // The references method returns an iterator of references, which
            // we can collect into a set after extracting each reference
            let iter = module
                .call_method1("references", (markdown,))?
                .try_iter()?
                .map(|item| item?.extract::<Reference>());

            // Collect references into a set
            Ok(References {
                inner: iter.collect::<PyResult<_>>()?,
            })
        })
    }
}
