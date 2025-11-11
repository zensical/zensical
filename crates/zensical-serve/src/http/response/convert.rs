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

//! HTTP response conversions.

use std::error::Error;
use std::result::Result;

use crate::http::Status;

use super::ext::ResponseExt;
use super::Response;

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<E> From<Result<Response, E>> for Response
where
    E: Error,
{
    /// Creates a response from a result.
    ///
    /// If the result is an error, the "500 Internal Server Error" status code
    /// is returned as a response, which indicates an unrecoverable error.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Error;
    /// use zensical_serve::http::{Response, Status};
    ///
    /// // Create response from error
    /// let err = Error::from_raw_os_error(1);
    /// let res = Response::from(Err(err));
    /// assert_eq!(res.status, Status::InternalServerError);
    /// ```
    fn from(result: Result<Response, E>) -> Self {
        result.unwrap_or_else(|_| {
            Response::from_status(Status::InternalServerError)
        })
    }
}
