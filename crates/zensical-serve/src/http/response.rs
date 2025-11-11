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

//! HTTP response.

use std::fmt;

use super::component::{Header, Status};

mod convert;
mod error;
mod ext;
mod headers;

pub use error::{Error, Result};
pub use ext::ResponseExt;
pub use headers::Headers;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP response.
///
/// While all members of this struct are public, there are also some dedicated
/// methods with identical names, providing a builder-like interface. However,
/// before creating a response using this struct directly, consider using the
/// [`ResponseExt`] trait, which provides several convenient constructors.
///
/// # Examples
///
/// ```
/// use zensical_serve::http::{Header, Response, Status};
///
/// // Create response
/// let res = Response::new()
///     .status(Status::Ok)
///     .header(Header::ContentType, "text/plain")
///     .header(Header::ContentLength, 13)
///     .body("Hello, world!");
/// ```
#[derive(Clone, Debug)]
pub struct Response {
    /// Response status.
    pub status: Status,
    /// Response headers.
    pub headers: Headers,
    /// Response body.
    pub body: Vec<u8>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Response {
    /// Creates a response.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Response;
    ///
    /// // Create response
    /// let res = Response::new();
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts the response into bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Header, Response, Status};
    ///
    /// // Create response
    /// let res = Response::new()
    ///    .status(Status::Ok)
    ///    .header(Header::ContentType, "text/plain")
    ///    .header(Header::ContentLength, 13)
    ///    .body("Hello, world!");
    ///
    /// // Convert response into bytes
    /// let bytes = res.into_bytes();
    /// ```
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        // Compute an estimate for the response size - we know that we need 8
        // bytes for the HTTP/1.1 prefix + 36 bytes for the status code + info,
        // both with 2 bytes for the CRLF at the end. Then, for each header, we
        // estimate an average size of 64 bytes per header (which might be more
        // than necessary, but that's okay), and reserve just enough space for
        // the body + 2 bytes for the CLRF that preceeds it.
        let capacity = (8 + 2)
            + 4 + 32 + 2 // fmt
            + self.headers.len() * 64 + 2 // fmt
            + self.body.len();

        // Create pre-sized buffer and append prefix and status
        let mut buffer = Vec::with_capacity(capacity);
        buffer.extend_from_slice(b"HTTP/1.1 ");
        buffer.extend_from_slice(self.status.to_string().as_bytes());
        buffer.extend_from_slice(b"\r\n");

        // Append all headers to buffer
        for (header, value) in &self.headers {
            buffer.extend_from_slice(header.name().as_bytes());
            buffer.extend_from_slice(b": ");
            buffer.extend_from_slice(value.as_bytes());
            buffer.extend_from_slice(b"\r\n");
        }

        // Append empty line and body to buffer, if given
        buffer.extend_from_slice(b"\r\n");
        if !self.body.is_empty() {
            buffer.extend_from_slice(&self.body);
        }

        // Return buffer
        buffer
    }
}

impl Response {
    /// Sets the status of the response.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Response, Status};
    ///
    /// // Create response and set status
    /// let res = Response::new()
    ///     .status(Status::Ok);
    /// ```
    #[inline]
    #[must_use]
    pub fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Adds a header to the response.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Header, Response};
    ///
    /// // Create response and add header
    /// let res = Response::new()
    ///     .header(Header::ContentType, "text/plain");
    /// ```
    #[inline]
    #[must_use]
    pub fn header<V>(mut self, header: Header, value: V) -> Self
    where
        V: ToString,
    {
        self.headers.insert(header, value);
        self
    }

    /// Sets the body of the response.
    ///
    /// __Warning__: Albeit the [`Header::ContentLength`] header is required in
    /// most cases, it's not automatically set when using this method, since it
    /// belongs to the low-level [`Response`] interface. Please consider using
    /// the much more convenient [`ResponseExt`] methods.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Response;
    ///
    /// // Create response and set body
    /// let res = Response::new()
    ///     .body("Hello, world!");
    /// ```
    #[inline]
    #[must_use]
    pub fn body<B>(mut self, body: B) -> Self
    where
        B: Into<Vec<u8>>,
    {
        self.body = body.into();
        self
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Default for Response {
    /// Creates a default response.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Response;
    ///
    /// // Create response
    /// let res = Response::default();
    /// ```
    #[inline]
    fn default() -> Self {
        Self {
            status: Status::Ok,
            headers: Headers::default(),
            body: Vec::default(),
        }
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Response {
    /// Formats the response for display.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HTTP/1.1 {}\r\n", self.status)?;
        write!(f, "{}\r\n", self.headers)?;
        write!(f, "[Body: {} bytes]\r\n", self.body.len())
    }
}
