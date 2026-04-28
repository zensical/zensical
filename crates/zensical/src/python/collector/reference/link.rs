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

//! Link reference.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::ops::Range;

use crate::python::Span;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Link kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// Link.
    Link,
    /// Image link.
    Image,
    /// Autolink.
    Autolink,
    /// Wikilink.
    Wikilink,
}

/// Link reference kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkReferenceKind {
    /// Link.
    Link,
    /// Image link.
    Image,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Link.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject)]
pub struct Link {
    /// Start offset.
    pub start: usize,
    /// End offset.
    pub end: usize,
    /// Link kind.
    pub kind: LinkKind,
    /// Span of visible link text.
    pub text: Span,
    /// Span of the link destination.
    pub href: Span,
}

/// Link reference.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject)]
pub struct LinkReference {
    /// Start offset.
    pub start: usize,
    /// End offset.
    pub end: usize,
    /// Link reference kind.
    pub kind: LinkReferenceKind,
    /// Span of visible link text.
    pub text: Span,
    /// Span of the link id.
    pub id: Span,
}

/// Link definition.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject)]
pub struct LinkDefinition {
    /// Start offset.
    pub start: usize,
    /// End offset.
    pub end: usize,
    /// Link kind.
    pub kind: LinkKind,
    /// Span of the link id.
    pub id: Span,
    /// Span of the link destination.
    pub href: Span,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<'a, 'py> FromPyObject<'a, 'py> for LinkKind {
    type Error = PyErr;

    /// Extracts a link kind from a Python object.
    #[inline]
    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        match obj.extract()? {
            "link" => Ok(Self::Link),
            "image" => Ok(Self::Image),
            "autolink" => Ok(Self::Autolink),
            "wikilink" => Ok(Self::Wikilink),
            _ => Err(PyValueError::new_err("Invalid kind")),
        }
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for LinkReferenceKind {
    type Error = PyErr;

    /// Extracts a link reference kind from a Python object.
    #[inline]
    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        match obj.extract()? {
            "link" => Ok(Self::Link),
            "image" => Ok(Self::Image),
            _ => Err(PyValueError::new_err("Invalid kind")),
        }
    }
}

// ----------------------------------------------------------------------------

impl From<Link> for Range<usize> {
    /// Creates a range from a link.
    #[inline]
    fn from(link: Link) -> Self {
        link.start..link.end
    }
}

impl From<LinkReference> for Range<usize> {
    /// Creates a range from a link reference.
    #[inline]
    fn from(link: LinkReference) -> Self {
        link.start..link.end
    }
}

impl From<LinkDefinition> for Range<usize> {
    /// Creates a range from a link definition.
    #[inline]
    fn from(link: LinkDefinition) -> Self {
        link.start..link.end
    }
}
