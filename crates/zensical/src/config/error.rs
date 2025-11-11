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

//! Configuration error.

use pyo3::exceptions::PyIOError;
use pyo3::PyErr;
use std::{io, result};
use thiserror::Error;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Configuration error.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),

    /// PyO3 error.
    #[error(transparent)]
    PyO3(#[from] pyo3::PyErr),
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl From<Error> for PyErr {
    /// Converts a configuration error to a [`PyErr`].
    #[inline]
    fn from(err: Error) -> PyErr {
        match err {
            Error::Io(err) => PyErr::new::<PyIOError, _>(err.to_string()),
            Error::PyO3(err) => err,
        }
    }
}

// ----------------------------------------------------------------------------
// Type aliases
// ----------------------------------------------------------------------------

/// Configuration result.
pub type Result<T = ()> = result::Result<T, Error>;
