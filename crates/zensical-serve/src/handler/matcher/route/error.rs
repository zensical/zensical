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

//! Route error.

use std::result;
use thiserror::Error;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Route error.
#[derive(Debug, Error)]
pub enum Error {
    /// Route must start with '/'.
    #[error("route must start with '/': {0}")]
    Relative(String),

    /// Route must not end with '/'.
    #[error("route must not end with '/': {0}")]
    Trailing(String),

    /// Route must not be empty.
    #[error("route must not be empty")]
    Empty,
}

// ----------------------------------------------------------------------------
// Type aliases
// ----------------------------------------------------------------------------

/// Route result.
pub type Result<T = ()> = result::Result<T, Error>;
