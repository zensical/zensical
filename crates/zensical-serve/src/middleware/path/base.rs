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

//! tbd

use std::borrow::Cow;
use std::str::FromStr;

use crate::handler::matcher::{Result, Route};
use crate::handler::Handler;
use crate::http::response::ResponseExt;
use crate::http::{Request, Response, Uri};
use crate::middleware::Middleware;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// tbd
pub struct BasePath {
    // Base path.
    base: Route,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl BasePath {
    /// Creates a base path middleware.
    pub fn new<P>(path: P) -> Result<Self>
    where
        P: AsRef<str>,
    {
        Route::from_str(path.as_ref())
            .map_err(Into::into)
            .map(|base| Self { base })
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for BasePath {
    /// Processes the given request.
    fn process(&self, mut req: Request, next: &dyn Handler) -> Response {
        let base = self.base.as_str();
        if base == "/" {
            return next.handle(req);
        }

        // 1. Handle root redirect if enabled
        if req.uri.path == "/" {
            return Response::redirect(base);
        }

        // 2. Strip prefix, if it exists
        if req.uri.path.starts_with(base) {
            req.uri = Uri::from_parts(
                Cow::Owned(req.uri.path.trim_start_matches(base).to_string()),
                req.uri.query,
            );
        }

        // Forward with modified request
        next.handle(req)
    }
}
