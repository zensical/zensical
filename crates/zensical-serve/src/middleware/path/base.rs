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
        if let Some(path) = strip_base_path(req.uri.path.as_ref(), base) {
            req.uri = Uri::from_parts(Cow::Owned(path), req.uri.query);
        }

        // Forward with modified request
        next.handle(req)
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn strip_base_path(path: &str, base: &str) -> Option<String> {
    if path == base {
        return Some("/".to_string());
    }

    path.strip_prefix(base)
        .filter(|rest| rest.starts_with('/'))
        .map(str::to_string)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::http::{Request, Response};

    #[test]
    fn strips_base_path_once() {
        let middleware = BasePath::new("/foo").expect("invariant");
        let req = Request::new().uri("/foo/food");

        let res = middleware.process(req, &|req: Request| {
            Response::new().body(req.uri.path.to_string())
        });

        assert_eq!(res.body, b"/food");
    }

    #[test]
    fn does_not_strip_non_segment_prefix() {
        let middleware = BasePath::new("/foo").expect("invariant");
        let req = Request::new().uri("/foobar");

        let res = middleware.process(req, &|req: Request| {
            Response::new().body(req.uri.path.to_string())
        });

        assert_eq!(res.body, b"/foobar");
    }
}
