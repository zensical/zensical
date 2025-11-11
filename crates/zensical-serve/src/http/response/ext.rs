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

use httpdate::fmt_http_date;
use std::fs;
use std::path::Path;

use crate::http::{Header, Status};

use super::{Response, Result};

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

/// Extension trait for the `Response` type providing additional functionality.
pub trait ResponseExt: Sized {
    /// Creates a response from a file.
    fn from_file<P>(path: P) -> Result<Response>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let mime = match path.extension().and_then(|ext| ext.to_str()) {
            Some("html" | "htm") => "text/html; charset=utf-8",
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("json") => "application/json",
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("svg") => "image/svg+xml",
            Some("ico") => "image/x-icon",
            Some("pdf") => "application/pdf",
            Some("mp4") => "video/mp4",
            Some("txt") => "text/plain; charset=utf-8",
            Some("xml") => "application/xml",
            _ => "application/octet-stream",
        };

        // Create the response from file
        fs::read(path).map_err(Into::into).and_then(|content| {
            let res = Response::new()
                .status(Status::Ok)
                .header(Header::ContentType, mime)
                .header(Header::ContentLength, content.len())
                .body(content);

            // Retrieve file metadata and add date, if applicable
            let meta = fs::metadata(path)?;
            let meta = meta.modified().map(fmt_http_date).ok();
            if let Some(date) = meta {
                Ok(res.header(Header::LastModified, date))
            } else {
                Ok(res)
            }
        })
    }

    /// Creates a response from plain text.
    fn from_text<S>(content: S) -> Response
    where
        S: Into<String>,
    {
        Response::new() // fmt
            .status(Status::Ok)
            .text(content)
    }

    /// Creates a response from a status code.
    ///
    /// This is a convenience method to create a response with a status code
    /// and a text body, particularly useful for error handling.
    #[must_use]
    fn from_status(status: Status) -> Response {
        Response::new() // fmt
            .status(status)
            .text(status.name())
    }

    /// Creates a redirect response.
    #[must_use]
    fn redirect<L>(location: L) -> Response
    where
        L: ToString,
    {
        Response::new()
            .status(Status::Found)
            .header(Header::Location, location)
            .header(Header::ContentLength, 0)
    }

    /// Sets the given text as the body of the response.
    fn text<S>(self, content: S) -> Response
    where
        S: Into<String>;
}

// ----------------------------------------------------------------------------
// Blanket implementations
// ----------------------------------------------------------------------------

impl ResponseExt for Response {
    /// Sets the given text as the body of the response.
    fn text<S>(self, content: S) -> Response
    where
        S: Into<String>,
    {
        let content = content.into();
        self.header(Header::ContentType, "text/plain; charset=utf-8")
            .header(Header::ContentLength, content.len())
            .body(content)
    }
}
