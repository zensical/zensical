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

//! Handler.

use std::fmt;

use super::http::response::ResponseExt;
use super::http::{Method, Request, Response, Status};

mod convert;
mod error;
pub mod matcher;
mod scope;
pub mod stack;

pub use convert::TryIntoHandler;
pub use error::{Error, Result};
pub use matcher::Matcher;
pub use scope::Scope;
pub use stack::Stack;

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

/// Handler.
///
/// Handlers represent the executable form of a request processing chain. Unlike
/// middlewares, which define composable layers of request processing, handlers
/// package those layers into a single unit of execution, always returning a
/// [`Response`] for every given [`Request`].
///
/// Note that a handler must be at the end of every request processing chain,
/// definitely answering the request with no next middleware to defer to.
pub trait Handler {
    /// Handles the given request.
    ///
    /// This method is invoked with a request and is required to return a
    /// response. It must be infallible and should not panic.
    ///
    /// # Examples
    ///
    /// This example shows how to implement a teapot handler responding with
    /// "418 I'm a Teapot" status code when the client tries to `GET /coffee`,
    /// while answering all other requests with "404 Not Found". Note that for
    /// routing, using a [`Router`][] is usually a better choice.
    ///
    /// [`Router`]: crate::router::Router
    ///
    /// ```
    /// use zensical_serve::handler::{Handler, Teapot};
    /// use zensical_serve::http::{Method, Request, Response, Status};
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .method(Method::Get)
    ///     .uri("/coffee");
    ///
    /// // Handle request with handler
    /// let res = Teapot.handle(req);
    /// assert_eq!(res.status, Status::ImATeapot);
    /// ```
    fn handle(&self, req: Request) -> Response;
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Fallback handler.
///
///
/// This handler always returns "404 Not Found", and is ideal as a default
/// fallback handler for middlewares like [`Stack`][] and [`Router`][].
///
/// [`Stack`]: crate::handler::Stack
/// [`Router`]: crate::router::Router
pub struct NotFound;

/// Teapot handler.
///
/// This handler responds with "418 I'm a Teapot" status code when the client
/// tries to `GET /coffee`, answering all other requests with "404 Not Found".
/// Besides that, it doesn't do anything, but it's a good choice to quickly
/// test starting a server or to use in examples.
pub struct Teapot;

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Handler for NotFound {
    /// Handles the given request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::{Handler, NotFound};
    /// use zensical_serve::http::{Method, Request, Status};
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .method(Method::Get)
    ///     .uri("/");
    ///
    /// // Handle request with handler
    /// let res = NotFound.handle(req);
    /// assert_eq!(res.status, Status::NotFound);
    /// ```
    #[inline]
    fn handle(&self, _req: Request) -> Response {
        Response::from_status(Status::NotFound)
    }
}

impl Handler for Teapot {
    /// Handles the given request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::{Handler, Teapot};
    /// use zensical_serve::http::{Method, Request, Status};
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .method(Method::Get)
    ///     .uri("/coffee");
    ///
    /// // Handle request with handler
    /// let res = Teapot.handle(req);
    /// assert_eq!(res.status, Status::ImATeapot);
    /// ```
    #[inline]
    fn handle(&self, req: Request) -> Response {
        if req.method == Method::Get && req.uri.path == "/coffee" {
            Response::from_status(Status::ImATeapot)
        } else {
            Response::from_status(Status::NotFound)
        }
    }
}

// ----------------------------------------------------------------------------

impl fmt::Debug for Box<dyn Handler> {
    /// Formats the handler for debugging.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Box<dyn Handler>")
    }
}

// ----------------------------------------------------------------------------
// Blanket implementations
// ----------------------------------------------------------------------------

impl<F, R> Handler for F
where
    F: Fn(Request) -> R,
    R: Into<Response>,
{
    #[inline]
    fn handle(&self, req: Request) -> Response {
        self(req).into()
    }
}
