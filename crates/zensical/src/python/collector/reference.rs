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
use std::fmt::{self, Debug};
use std::slice::Iter;
use std::str::FromStr;
use zrx::stream::Value;

mod footnote;
mod link;

pub use footnote::{FootnoteDefinition, FootnoteReference};
pub use link::{Link, LinkDefinition, LinkReference};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Reference.
///
/// Note that the order of variants is significant, as it determines the order
/// in which references are converted from Python objects. We must ensure that
/// footnote definitions are checked before footnote references, since the
/// later is a subset of the former in terms of their fields.
#[derive(Clone, PartialEq, Eq, FromPyObject)]
pub enum Reference {
    /// Link or image.
    Link(Link),
    /// Link definition.
    LinkDefinition(LinkDefinition),
    /// Link reference.
    LinkReference(LinkReference),
    /// Footnote definition.
    FootnoteDefinition(FootnoteDefinition),
    /// Footnote reference.
    FootnoteReference(FootnoteReference),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Reference set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct References {
    /// Markdown.
    markdown: String,
    /// Inner set of references.
    inner: Vec<Reference>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl References {
    /// Returns the Markdown from which the references were extracted.
    #[inline]
    pub fn markdown(&self) -> &str {
        &self.markdown
    }

    /// Returns an iterator over the references.
    #[inline]
    pub fn iter(&self) -> Iter<'_, Reference> {
        self.inner.iter()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for References {}

// ----------------------------------------------------------------------------

impl FromStr for References {
    type Err = PyErr;

    /// Parses references from Markdown.
    #[inline]
    fn from_str(markdown: &str) -> PyResult<Self> {
        Python::attach(|py| {
            let module = py.import("zensical.collectors")?;

            // The references method returns an iterator of references
            let iter = module
                .call_method1("references", (markdown.as_bytes(),))?
                .try_iter()?
                .map(|item| item?.extract::<Reference>());

            // Collect references into a reference set
            Ok(References {
                markdown: markdown.to_string(),
                inner: iter.collect::<PyResult<_>>()?,
            })
        })
    }
}

// ----------------------------------------------------------------------------

impl<'a> IntoIterator for &'a References {
    type Item = &'a Reference;
    type IntoIter = Iter<'a, Reference>;

    /// Creates an iterator over the references.
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

// ----------------------------------------------------------------------------

impl Debug for Reference {
    /// Formats the reference for debugging.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Reference::Link(link) => Debug::fmt(link, f),
            Reference::LinkDefinition(link) => Debug::fmt(link, f),
            Reference::LinkReference(link) => Debug::fmt(link, f),
            Reference::FootnoteDefinition(footnote) => Debug::fmt(footnote, f),
            Reference::FootnoteReference(footnote) => Debug::fmt(footnote, f),
        }
    }
}
