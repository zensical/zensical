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

//! Stack.

use crate::handler::{Handler, NotFound};
use crate::http::{Request, Response};
use crate::middleware::Middleware;

use super::matcher::Matcher;

mod builder;
mod factory;

pub use builder::Builder;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Stack.
///
/// Stacks allow to compose and unify multiple middlewares into one, passing the
/// request from one middleware to the next, until the last one is reached. Each
/// middleware can modify the request and/or response, short-circuit processing,
/// or even return a response directly. This allows for creating complex request
/// processing pipelines, where each middleware can handle a specific aspect,
/// of the pipeline, e.g., serving of static files, caching, etc.
///
/// Any implementor of [`TryIntoMiddleware`][] can be added to the stack, which
/// includes [`Stack`] itself. This allows to create a tree of middlewares, as
/// well as middlewares scoped to certain paths using a [`Router`][], which can
/// contain further middlewares.
///
/// It's middlewares all the way down.
///
/// [`Router`]: crate::router::Router
/// [`TryIntoMiddleware`]: crate::middleware::TryIntoMiddleware
///
/// # Examples
///
/// ```
/// # use std::error::Error;
/// # fn main() -> Result<(), Box<dyn Error>> {
/// use zensical_serve::handler::{Handler, Stack, TryIntoHandler};
/// use zensical_serve::http::{Method, Request, Response, Status};
/// use zensical_serve::middleware::Middleware;
///
/// // Create stack with middleware
/// let stack = Stack::new()
///     .with(|req: Request, next: &dyn Handler| {
///         if req.method == Method::Get && req.uri.path == "/coffee" {
///             Response::new().status(Status::ImATeapot)
///         } else {
///             next.handle(req)
///         }
///     })
///     .try_into_handler()?;
///
/// // Create request
/// let req = Request::new()
///     .method(Method::Get)
///     .uri("/coffee");
///
/// // Handle request with stack
/// let res = stack.handle(req);
/// assert_eq!(res.status, Status::ImATeapot);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct Stack {
    /// Middlewares.
    middlewares: Vec<Box<dyn Middleware>>,
    /// Matcher, optional.
    ///
    /// When a stack is added to a [`Router`][], the matcher ensures execution
    /// only happens if the router's base path matches the request path as a
    /// prefix. Stacks created outside of routers don't have an associated
    /// matcher, and thus match any request passed to [`Stack::process`].
    ///
    /// [`Router`]: crate::router::Router
    matcher: Option<Matcher>,
}

/// Stack handler.
///
/// The stack handler keeps track of all middlewares that haven't been invoked
/// yet, i.e., are next in line to be called, and a reference to the handler
/// which should be invoked, when no middleware is left. The handler is passed
/// to [`Stack::process`], which is the implementation of [`Middleware`].
struct StackHandler<'a> {
    /// Remaining middlewares.
    middlewares: &'a [Box<dyn Middleware>],
    /// Next handler.
    next: &'a dyn Handler,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Stack {
    /// Creates a stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::Stack;
    ///
    /// // Create stack
    /// let stack = Stack::new();
    /// ```
    #[allow(clippy::new_ret_no_self)]
    #[must_use]
    pub fn new() -> Builder {
        // Note that we deliberately return a builder here, and not a stack.
        // While it would be idiomatic to call this method `builder` then, we
        // chose to use `new` for consistency with routers and possibly other
        // implementors that convert into stacks.
        Builder::new()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for Stack {
    /// Processes the given request.
    ///
    /// This method starts with the first middleware, and passes the request
    /// from one middleware to the next. If no middleware is left, the handler
    /// is invoked. Note that middlewares can also pass the request to the next
    /// middleware and modify the returned response.
    ///
    /// In case the stack is used as part of a [`Router`][], prior to invoking
    /// the first middleware, we check if the router's base path matches the
    /// request path as a prefix. If it doesn't, the request is passed to the
    /// next handler immediately.
    ///
    /// [`Router`]: crate::router::Router
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::{Handler, NotFound, Scope, Stack};
    /// use zensical_serve::http::{Method, Request, Response, Status};
    /// use zensical_serve::middleware::{Middleware, TryIntoMiddleware};
    ///
    /// // Create scope
    /// let scope = Scope::default();
    ///
    /// // Create stack with middleware
    /// let stack = Stack::new()
    ///     .with(|req: Request, next: &dyn Handler| {
    ///         if req.method == Method::Get && req.uri.path == "/coffee" {
    ///             Response::new().status(Status::ImATeapot)
    ///         } else {
    ///             next.handle(req)
    ///         }
    ///     })
    ///     .try_into_middleware(&scope)?;
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .method(Method::Get)
    ///     .uri("/coffee");
    ///
    /// // Handle request with stack
    /// let res = stack.process(req, &NotFound);
    /// assert_eq!(res.status, Status::ImATeapot);
    /// # Ok(())
    /// # }
    /// ```
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        if let Some(matcher) = &self.matcher {
            let path = req.uri.path.trim_end_matches('/');

            // Forward to next handler if path doesn't match
            if matcher.resolve(path).is_none() {
                return next.handle(req);
            }
        }

        // Create stack handler
        let handler = StackHandler {
            middlewares: &self.middlewares,
            next,
        };

        // Handle request
        handler.handle(req)
    }
}

// ----------------------------------------------------------------------------

impl Handler for Stack {
    /// Handles the given request, passing it through the entire stack.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::{Handler, Stack, TryIntoHandler};
    /// use zensical_serve::http::{Method, Request, Response, Status};
    ///
    /// // Create stack with middleware
    /// let stack = Stack::new()
    ///     .with(|req: Request, next: &dyn Handler| {
    ///         if req.method == Method::Get && req.uri.path == "/coffee" {
    ///             Response::new().status(Status::ImATeapot)
    ///         } else {
    ///             next.handle(req)
    ///         }
    ///     })
    ///     .try_into_handler()?;
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .method(Method::Get)
    ///     .uri("/coffee");
    ///
    /// // Handle request with stack
    /// let res = stack.handle(req);
    /// assert_eq!(res.status, Status::ImATeapot);
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    fn handle(&self, req: Request) -> Response {
        self.process(req, &NotFound)
    }
}

impl Handler for StackHandler<'_> {
    /// Handles the given request.
    ///
    /// This method is called by the stack to process the request. It checks
    /// if there are any middlewares left, and if so, it removes the first one,
    /// creates a new stack handler with the remaining middlewares, and invokes
    /// it. If no middlewares are left, the next handler is invoked.
    fn handle(&self, req: Request) -> Response {
        match self.middlewares {
            [] => self.next.handle(req),
            [middleware, middlewares @ ..] => {
                let next = StackHandler { middlewares, next: self.next };
                middleware.process(req, &next)
            }
        }
    }
}

// ----------------------------------------------------------------------------

impl FromIterator<Box<dyn Middleware>> for Stack {
    /// Creates a stack from an iterator.
    ///
    /// Note that this is primarily intended for internal use, as stacks are
    /// usually created through method chaining via [`Builder::with`].
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Box<dyn Middleware>>,
    {
        Self {
            middlewares: Vec::from_iter(iter),
            matcher: None,
        }
    }
}
