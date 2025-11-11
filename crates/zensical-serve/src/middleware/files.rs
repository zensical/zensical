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

//! Middleware for serving static files.

use httpdate::parse_http_date;
use std::fs;
use std::io::Result;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::handler::Handler;
use crate::http::response::ResponseExt;
use crate::http::{Header, Method, Request, Response, Status};
use crate::middleware::Middleware;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Middleware for serving static files.
///
/// Since the static files middleware is fallible during construction, we might
/// consider implementing [`TryIntoMiddleware`] for it later on.
///
/// [`TryIntoMiddleware`]: crate::middleware::TryIntoMiddleware
pub struct StaticFiles {
    /// Base path.
    base: PathBuf,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl StaticFiles {
    /// Creates a middleware for serving static files.
    pub fn new<P>(path: P) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        path.canonicalize().map(|base| Self { base })
    }

    /// Handle fallback cases (file not found, wrong method, etc.)
    fn fallback(&self, req: Request, next: &dyn Handler) -> Response {
        let res = next.handle(req);

        // In case the path was not found, try to load `404.html`
        if res.status == Status::NotFound {
            let full = self.base.join("404.html");
            if let Ok(res) = Response::from_file(full) {
                return res;
            }
        }

        // Otherwise, return original request
        res
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for StaticFiles {
    /// Processes the given request.
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        if !matches!(req.method, Method::Get | Method::Head) {
            return self.fallback(req, next);
        }

        // Remove leading slash from path. In case the path ends with a slash,
        // add "index.html", so we can correctly resolve the associated file
        let path = PathBuf::from(req.uri.path.trim_start_matches('/'));
        let mut full = self.base.join(&path);
        if req.uri.path.ends_with('/') {
            full.push("index.html");
        }

        // Attempt to load file, or delegate to fallback
        let Ok(mut res) = Response::from_file(&full) else {
            return self.fallback(req, next);
        };

        // Ensure a date is always set, as required by HTTP/1.1
        res.headers
            .insert(Header::Date, httpdate::fmt_http_date(SystemTime::now()));

        // In case we received a head request, remove body - we should rather
        // make this more granular by just checking for the file
        if req.method == Method::Head {
            return res.body([]);
        }

        // Try to obtain and parse header from request
        let option = req.headers.get(Header::IfModifiedSince);
        let Ok(header) = option.map(parse_http_date).transpose() else {
            return res;
        };

        // In case we can both extract the date from the header and the file
        // system lookup is successful, check if we can just return a 304
        if let (Some(date), Ok(meta)) = (header, fs::metadata(full)) {
            if let Ok(mut last) = meta.modified() {
                // Subtract one second to account for rounding issues
                last -= Duration::from_secs(1);
                if date >= last {
                    return Response::new()
                        .status(Status::NotModified)
                        .header(Header::ContentLength, 0);
                }
            }
        }

        // Otherwise just return response
        res
    }
}
