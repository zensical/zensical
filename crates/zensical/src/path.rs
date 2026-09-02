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

//! Paths with explicit source, site, and filesystem-root domains.
//!
//! Logical paths use a canonical, platform-independent `/` representation.
//! They deliberately exclude URL semantics such as queries, fragments,
//! schemes, percent encoding, empty routes, and trailing slashes.

use thiserror::Error;

mod relative;
mod root;
mod site;
mod source;

pub use root::{OutputRoot, RootError, SourceRoot};
pub use site::SitePath;
pub use source::SourcePath;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Error returned when a logical path is not canonical and relative.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PathError {
    /// The path contains no components.
    #[error("logical path must not be empty")]
    Empty,
    /// The path starts at a filesystem root or platform prefix.
    #[error("logical path must be relative: {0}")]
    Absolute(String),
    /// The path uses a platform-dependent backslash separator.
    #[error("logical path must use forward slashes: {0}")]
    Backslash(String),
    /// The path contains an empty or current-directory component.
    #[error("logical path must use its canonical spelling: {0}")]
    NonCanonical(String),
    /// The path contains a parent-directory component.
    #[error("logical path must not contain parent traversal: {0}")]
    Parent(String),
    /// The physical path contains a component that is not valid UTF-8.
    #[error("logical path must be valid UTF-8")]
    NonUtf8,
    /// The path contains a NUL byte and cannot name a filesystem entry.
    #[error("logical path must not contain NUL bytes")]
    Nul,
}
