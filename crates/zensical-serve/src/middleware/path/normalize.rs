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

//! Middleware for request path normalization.

use std::path::Path;

use crate::handler::Handler;
use crate::http::response::ResponseExt;
use crate::http::{Request, Response, Uri};
use crate::middleware::Middleware;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Trailing slash behavior.
///
/// This behavior determines how the [`NormalizePath`] middleware normalizes
/// request paths, appending or removing a trailing slash to them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingSlash {
    /// Append trailing slash.
    Append,
    /// Remove trailing slash.
    Remove,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Middleware for request path normalization.
///
/// This middleware normalizes the request path according to the configured
/// trailing slash behavior. Using [`NormalizePath::default`] is recommended,
/// as it appends a trailing slash in case the requested resource is not a
/// file allowing the server to automatically serve directory indexes.
///
/// # Examples
///
/// ```
/// # use std::error::Error;
/// # fn main() -> Result<(), Box<dyn Error>> {
/// use zensical_serve::handler::{Handler, Stack, TryIntoHandler};
/// use zensical_serve::http::{Header, Method, Request, Status};
/// use zensical_serve::middleware::NormalizePath;
///
/// // Create stack with middleware
/// let stack = Stack::new()
///     .with(NormalizePath::default())
///     .try_into_handler()?;
///
/// // Create request
/// let req = Request::new()
///     .method(Method::Get)
///     .uri("/coffee");
///
/// // Handle request with stack
/// let res = stack.handle(req);
/// assert_eq!(res.status, Status::Found);
/// assert_eq!(res.headers.get(Header::Location), Some("/coffee/"));
/// # Ok(())
/// # }
/// ```
pub struct NormalizePath {
    /// Trailing slash behavior.
    slash: TrailingSlash,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl NormalizePath {
    /// Creates a middleware for request path normalization.
    ///
    /// Consider using [`NormalizePath::default`] in case you want to use the
    /// recommended default behavior of appending a trailing slash, which is
    /// required for directory index serving, i.e., returning the `index.html`
    /// file in case a directory is requested.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::middleware::{NormalizePath, TrailingSlash};
    ///
    /// // Create middleware
    /// let middleware = NormalizePath::new(TrailingSlash::Append);
    /// ```
    #[must_use]
    pub fn new(slash: TrailingSlash) -> Self {
        Self { slash }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for NormalizePath {
    /// Processes the given request.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::{NotFound, Stack};
    /// use zensical_serve::http::{Header, Request, Status};
    /// use zensical_serve::middleware::{Middleware, NormalizePath};
    ///
    /// // Create middleware
    /// let middleware = NormalizePath::default();
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .uri("/coffee");
    ///
    /// // Handle request with middleware
    /// let res = middleware.process(req, &NotFound);
    /// assert_eq!(res.status, Status::Found);
    /// assert_eq!(res.headers.get(Header::Location), Some("/coffee/"));
    /// # Ok(())
    /// # }
    /// ```
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        // Create a path from a string reference, as it allows us to efficiently
        // check if it has an extension, regardless of which slashes are used in
        // file system paths. If it doesn't have an extension, it's either a
        // directory on the filesystem, or may point to a registered route.
        let path = Path::new(req.uri.path.as_ref());
        if req.uri.path == "/" || path.extension().is_some() {
            return next.handle(req);
        }

        // Depending on the trailing slash behavior, we need to check if the
        // request path has a trailing slash. If it does not match the desired
        // behavior, we send a redirect response to the client, instructing it
        // to request the resource with the correct path. We deliberately do
        // not send a "301 Moved Permanently" status code, as this would cause
        // the client to cache the redirect indefinitely, which is not what we
        // want. Additionally, this allows us to detect when links point to
        // non-canonical URLs, e.g., to automatically fix them in the sources.
        match (self.slash, req.uri.path.ends_with('/')) {
            // Append slash and return redirect
            (TrailingSlash::Append, false) => {
                let mut path = req.uri.path.into_owned();
                path.push('/');
                Response::redirect(Uri::from_parts(path, req.uri.query))
            }

            // Remove slash and return redirect
            (TrailingSlash::Remove, true) => {
                let mut path = req.uri.path.into_owned();
                path.pop();
                Response::redirect(Uri::from_parts(path, req.uri.query))
            }

            // Pass through all other requests
            _ => next.handle(req),
        }
    }
}

// ----------------------------------------------------------------------------

impl Default for NormalizePath {
    /// Creates a default middleware for request path normalization.
    ///
    /// By default, the middleware appends a trailing slash to the request path,
    /// and returns it as part of a "302 Found" response. It's the recommended
    /// default behavior, as it allows the server to handle requests for
    /// directories and files in a consistent manner.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::middleware::NormalizePath;
    ///
    /// // Create middleware
    /// let middleware = NormalizePath::default();
    /// ```
    fn default() -> Self {
        Self { slash: TrailingSlash::Append }
    }
}
