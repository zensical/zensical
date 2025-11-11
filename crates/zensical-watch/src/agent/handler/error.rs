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

//! File handler error.

use crossbeam::channel::{RecvError, SendError};
use std::{io, result};
use thiserror::Error;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// File handler error.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),

    /// Notify error.
    #[error(transparent)]
    Notify(#[from] notify::Error),

    /// Walk directory error.
    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),

    /// Channel disconnected.
    #[error("channel disconnected")]
    Disconnected,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<T> From<SendError<T>> for Error {
    /// Creates an error from a send error.
    #[inline]
    fn from(_: SendError<T>) -> Self {
        Error::Disconnected
    }
}

impl From<RecvError> for Error {
    /// Creates an error from a receive error.
    #[inline]
    fn from(_: RecvError) -> Self {
        Error::Disconnected
    }
}

// ----------------------------------------------------------------------------
// Type aliases
// ----------------------------------------------------------------------------

/// File handler result.
pub type Result<T = ()> = result::Result<T, Error>;
