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

//! Stack builder.

use std::str::FromStr;

use crate::handler::matcher::{Matcher, Route};
use crate::handler::{Error, Result, Scope, TryIntoHandler};
use crate::middleware::{Middleware, TryIntoMiddleware};

use super::factory::Factory;
use super::Stack;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Stack builder.
#[derive(Debug)]
pub struct Builder {
    /// Middleware factories.
    middlewares: Vec<Box<dyn Factory>>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Builder {
    /// Creates a stack builder.
    ///
    /// Note that the canonical way to create a [`Stack`] is to invoke the
    /// [`Stack::new`] method, which creates an instance of [`Builder`].
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::stack::Builder;
    ///
    /// // Create stack builder
    /// let builder = Builder::new();
    /// ```
    #[allow(clippy::new_without_default)]
    #[must_use]
    pub fn new() -> Self {
        Self { middlewares: Vec::new() }
    }

    /// Extends the stack with the given middleware.
    ///
    /// Anything that can be converted into a [`Middleware`] can be added to
    /// the stack, including middlewares, routers, stacks and closures.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::{Handler, Stack};
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
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn with<T>(mut self, middleware: T) -> Self
    where
        T: TryIntoMiddleware,
    {
        self.add(middleware);
        self
    }

    /// Adds a middleware to the stack.
    ///
    /// Note that [`Builder::with`] is the recommended way to compose stacks
    /// from middlewares. This method is primarily needed by the [`Router`][].
    ///
    /// [`Router`]: crate::router::Router
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::{Handler, Stack};
    /// use zensical_serve::http::{Method, Request, Response, Status};
    ///
    /// // Create stack and add middleware
    /// let mut stack = Stack::new();
    /// stack.add(|req: Request, next: &dyn Handler| {
    ///     if req.method == Method::Get && req.uri.path == "/coffee" {
    ///         Response::new().status(Status::ImATeapot)
    ///     } else {
    ///         next.handle(req)
    ///     }
    /// });
    /// ```
    pub fn add<T>(&mut self, middleware: T)
    where
        T: TryIntoMiddleware,
    {
        self.middlewares.push(Box::new(|scope: &Scope| {
            middleware
                .try_into_middleware(scope)
                .map(|middleware| Box::new(middleware) as Box<dyn Middleware>)
        }));
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl TryIntoMiddleware for Builder {
    type Output = Stack;

    /// Attempts to convert the stack into a middleware.
    ///
    /// # Errors
    ///
    /// In case conversion fails, an [`Error`] is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::{Handler, Scope, Stack};
    /// use zensical_serve::http::{Method, Request, Response, Status};
    /// use zensical_serve::middleware::TryIntoMiddleware;
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
    /// # Ok(())
    /// # }
    /// ```
    fn try_into_middleware(self, scope: &Scope) -> Result<Self::Output> {
        let route = scope.route.as_ref();

        // If the stack is part of a router, we create a matcher that checks if
        // the router's base path matches the request path as a prefix
        let matcher = route
            .map(|base| -> Result<_> {
                let mut matcher = Matcher::new();
                let rest = Route::from_str("/{*rest}")
                    .map_err(|err| Error::Matcher(err.into()))?;

                // Middlewares do not receive path parameters, which is why we
                // just use a wildcard to implement prefix matching on paths
                matcher
                    .add(base.append(rest), ())
                    .map_err(Into::into)
                    .map(|()| matcher)
            })
            .transpose()?;

        // Create and collect middlewares into a stack
        let iter = self.middlewares.into_iter().map(|f| f(scope));
        iter.collect::<Result<_>>()
            .map(|middlewares| Stack { middlewares, matcher })
    }
}

impl TryIntoHandler for Builder {
    type Output = Stack;

    /// Attempts to convert the stack into a handler.
    ///
    /// This method is equivalent to calling [`Stack::try_into_middleware`]
    /// with [`Scope::default`], scoping all middlewares to `/`.
    ///
    /// # Errors
    ///
    /// In case conversion fails, an [`Error`] is returned.
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
    /// # Ok(())
    /// # }
    /// ```
    fn try_into_handler(self) -> Result<Self::Output> {
        let scope = Scope::default();
        self.try_into_middleware(&scope)
    }
}
