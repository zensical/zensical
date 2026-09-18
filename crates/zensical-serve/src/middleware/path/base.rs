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

//! Middleware for serving a site below a base path.

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

/// Middleware that redirects and removes a configured request base path.
pub struct BasePath {
    /// Base path removed from matching requests.
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

        // The configured site root is a directory, even if it contains dots
        if req.uri.path == "/" || req.uri.path == base {
            let path = format!("{base}/");
            return Response::redirect(Uri::from_parts(path, req.uri.query));
        }

        // Strip prefix, if it exists
        if let Some(path) = strip_base_path(req.uri.path.as_ref(), base) {
            req.uri = Uri::from_parts(Cow::Owned(path), req.uri.query);
        }

        // Forward with modified request
        next.handle(req)
    }
}

// ----------------------------------------------------------------------------
// Functions
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
    use crate::handler::{NotFound, Stack, TryIntoHandler};
    use crate::http::{Header, Request, Response, Status, Uri};
    use crate::middleware::{Middleware, NormalizePath};

    use super::BasePath;

    #[test]
    fn redirects_site_root_with_trailing_slash() {
        for base in ["/company.pages", "/group/company.pages", "/group/project"]
        {
            let stack = Stack::new()
                .with(NormalizePath::default())
                .with(BasePath::new(base).expect("invariant"))
                .try_into_handler()
                .expect("invariant");

            for path in ["/", base] {
                for query in ["", "q=search"] {
                    let req = Request::new().uri(Uri::from_parts(path, query));
                    let res = stack.process(req, &NotFound);
                    let location =
                        Uri::from_parts(format!("{base}/"), query).to_string();

                    assert_eq!(res.status, Status::Found);
                    assert_eq!(
                        res.headers.get(Header::Location),
                        Some(location.as_str())
                    );

                    let req = Request::new().uri(location.as_str());
                    let res = stack.process(req, &|req: Request| {
                        Response::new().body(req.uri.to_string())
                    });

                    assert_eq!(res.status, Status::Ok);
                    assert_eq!(
                        res.body,
                        Uri::from_parts("/", query).to_string().as_bytes()
                    );
                }
            }
        }
    }

    #[test]
    fn preserves_asset_paths_below_dotted_base() {
        let middleware =
            BasePath::new("/group/company.pages").expect("invariant");
        let req =
            Request::new().uri("/group/company.pages/assets/main.css?v=123");

        let res = middleware.process(req, &|req: Request| {
            Response::new().body(req.uri.to_string())
        });

        assert_eq!(res.status, Status::Ok);
        assert_eq!(res.body, b"/assets/main.css?v=123");
    }

    #[test]
    fn preserves_unprefixed_site_root() {
        let middleware = BasePath::new("/").expect("invariant");
        let req = Request::new().uri("/?q=search");

        let res = middleware.process(req, &|req: Request| {
            Response::new().body(req.uri.to_string())
        });

        assert_eq!(res.status, Status::Ok);
        assert_eq!(res.body, b"/?q=search");
    }

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
