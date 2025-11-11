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

//! Action.

use std::fmt;

use crate::http::{Request, Response};
use crate::router::Params;

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

/// Action.
///
/// If a route is matched, the registered action is called with the [`Request`]
/// and [`Params`], which were extracted from the route, if any. Currently, an
/// action is required to always return a [`Response`], which means it will be
/// considered the end of the processing chain.
///
/// Of course it's possible to add middlewares after routes, but it's important
/// to understand that they are only executed if none of the routes matched.
pub trait Action: 'static {
    /// Handles the given request with parameters.
    ///
    /// This method is invoked with a request and parameters and is required to
    /// return a response. It must be infallible and should not panic. Note that
    /// actions are rather an internal concept, which are automatically created
    /// when registering routes in a [`Router`].
    ///
    /// # Examples
    ///
    /// This example shows how to implement a teapot route responding with
    /// "418 I'm a Teapot" status code when the client tries to `GET /coffee`,
    /// while answering all other requests with "404 Not Found". This example
    /// uses a [`Router`], the idiomatic method to implement routing.
    ///
    /// [`Router`]: crate::router::Router
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::{Handler, TryIntoHandler};
    /// use zensical_serve::http::{Method, Request, Response, Status};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .get("/coffee", |req: Request, params: Params| {
    ///         Response::new().status(Status::ImATeapot)
    ///     })
    ///     .try_into_handler()?;
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .method(Method::Get)
    ///     .uri("/coffee");
    ///
    /// // Handle request with router
    /// let res = router.handle(req);
    /// assert_eq!(res.status, Status::ImATeapot);
    /// # Ok(())
    /// # }
    /// ```
    fn handle(&self, req: Request, params: Params) -> Response;
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl fmt::Debug for Box<dyn Action> {
    /// Formats the action for debugging.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Box<dyn Action>")
    }
}

// ----------------------------------------------------------------------------
// Blanket implementations
// ----------------------------------------------------------------------------

impl<F, R> Action for F
where
    F: Fn(Request, Params) -> R + 'static,
    R: Into<Response>,
{
    #[inline]
    fn handle(&self, req: Request, params: Params) -> Response {
        self(req, params).into()
    }
}
