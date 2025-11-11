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

//! Router.

use std::str::FromStr;

use super::handler::matcher::Route;
use super::handler::stack::{self, Stack};
use super::handler::{Error, Result, Scope, TryIntoHandler};
use super::http::Method;
use super::middleware::{Middleware, TryIntoMiddleware};

// Re-export for convenient usage with routers
pub use super::handler::matcher::Params;

mod action;
mod routes;

pub use action::Action;
use routes::Routes;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Builder.
///
/// Routers are built using a combination of stacks and routes, which can be
/// combined into a single stack when converting with [`TryIntoMiddleware`].
#[derive(Debug)]
enum Builder {
    /// Stack builder.
    Stack(stack::Builder),
    /// Routes builder.
    Routes(routes::Builder),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Router.
///
/// Routers allow to scope specific actions to a combination of HTTP methods
/// and path patterns, making them essentially a specialization of [`Stack`].
/// Additionally, routers allow for the addition of middlewares, which are
/// grouped into stacks, and can be defined before and after routes.
#[derive(Debug)]
pub struct Router {
    /// Builders.
    builders: Vec<Builder>,
    /// Base path.
    path: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Router {
    /// Creates a router.
    ///
    /// The given path is prepended to all routes that are created as part of
    /// the router. Using [`Router::default`] is equivalent to passing `/`.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::router::Router;
    ///
    /// // Create router
    /// let router = Router::new("/");
    /// ```
    pub fn new<P>(path: P) -> Self
    where
        P: Into<String>,
    {
        Self {
            builders: Vec::new(),
            path: path.into(),
        }
    }

    /// Adds a `GET` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .get("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn get<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Get, path, action)
    }

    /// Adds a `POST` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .post("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn post<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Post, path, action)
    }

    /// Adds a `PUT` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .put("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn put<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Put, path, action)
    }

    /// Adds a `DELETE` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .delete("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn delete<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Delete, path, action)
    }

    /// Adds a `PATCH` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .patch("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn patch<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Patch, path, action)
    }

    /// Adds a `HEAD` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .head("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn head<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Head, path, action)
    }

    /// Adds a `OPTIONS` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .options("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn options<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Options, path, action)
    }

    /// Adds a `TRACE` route to the router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .trace("/", |req: Request, params: Params| {
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn trace<P, A>(self, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        self.route(Method::Trace, path, action)
    }

    /// Adds a middleware to the router.
    ///
    /// Middlewares can be added at any point in the router stack, including
    /// before or after routes. This allows for flexible routing and middleware
    /// combinations, as routes are themselves combines into middlewares, when
    /// the router is converted into a middleware.
    ///
    /// Anything that can be converted into a [`Middleware`] can be added to
    /// the stack, including middlewares, routers, stacks and closures.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::Handler;
    /// use zensical_serve::http::{Method, Request, Response, Status};
    /// use zensical_serve::router::Router;
    ///
    /// // Create router with middleware
    /// let stack = Router::default()
    ///     .with(|req: Request, next: &dyn Handler| {
    ///         if req.method == Method::Get && req.uri.path == "/coffee" {
    ///             Response::new().status(Status::ImATeapot)
    ///         } else {
    ///             next.handle(req)
    ///         }
    ///     });
    /// ```
    #[must_use]
    pub fn with<T>(mut self, middleware: T) -> Self
    where
        T: TryIntoMiddleware,
    {
        // Consecutive middlewares are grouped into stacks, so we must ensure
        // that the current item is a stack builder, and add the middleware
        if let Some(Builder::Stack(builder)) = self.builders.last_mut() {
            builder.add(middleware);
        } else {
            let mut builder = Stack::new();
            builder.add(middleware);
            self.builders.push(Builder::Stack(builder));
        }

        // Return self for chaining
        self
    }

    /// Adds a route to the router.
    fn route<P, A>(mut self, method: Method, path: P, action: A) -> Self
    where
        P: Into<String>,
        A: Action,
    {
        // Consecutive routes are grouped into matchers, so we must ensure
        // that the current item is a routes builder, and add the route
        if let Some(Builder::Routes(builder)) = self.builders.last_mut() {
            builder.add(method, path, action);
        } else {
            let mut builder = Routes::builder();
            builder.add(method, path, action);
            self.builders.push(Builder::Routes(builder));
        }

        // Return self for chaining
        self
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl TryIntoMiddleware for Router {
    type Output = Stack;

    /// Attempts to convert the router into a middleware.
    ///
    /// # Errors
    ///
    /// In case conversion fails, an [`Error`][] is returned.
    ///
    /// [`Error`]: crate::handler::Error
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::Scope;
    /// use zensical_serve::http::{Request, Response, Status};
    /// use zensical_serve::middleware::TryIntoMiddleware;
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create scope
    /// let scope = Scope::default();
    ///
    /// // Create router and convert into middleware
    /// let router = Router::default()
    ///     .get("/coffee", |req: Request, params: Params| {
    ///         Response::new().status(Status::ImATeapot)
    ///     })
    ///     .try_into_middleware(&scope)?;
    /// # Ok(())
    /// # }
    /// ```
    fn try_into_middleware(self, scope: &Scope) -> Result<Self::Output> {
        let path = Route::from_str(&self.path)
            .map_err(|err| Error::Matcher(err.into()))?;

        // Join the parent scope with the scope derived from the router's base
        // path, which is then used for constructing routes and stacks
        let scope = scope.join(path);

        // Transform builders into middlewares - routers can host builders for
        // stacks and routes, both of which are converted into middlewares, and
        // then collected into a stack that can be converted into a handler.
        // Routes are validated and checked during conversion.
        let iter = self.builders.into_iter().map(|item| match item {
            // Convert stack into middleware
            Builder::Stack(builder) => builder
                .try_into_middleware(&scope)
                .map(|middleware| Box::new(middleware) as Box<dyn Middleware>),

            // Convert routes into middleware
            Builder::Routes(builder) => builder
                .try_into_middleware(&scope)
                .map(|middleware| Box::new(middleware) as Box<dyn Middleware>),
        });

        // Collect middlewares into a stack
        iter.collect()
    }
}

impl TryIntoHandler for Router {
    type Output = Stack;

    /// Attempts to convert the router into a handler.
    ///
    /// This method is equivalent to calling [`Router::try_into_middleware`]
    /// with [`Scope::default`], scoping all middlewares and routes to `/`.
    ///
    /// # Errors
    ///
    /// In case conversion fails, an [`Error`][] is returned.
    ///
    /// [`Error`]: crate::handler::Error
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::TryIntoHandler;
    /// use zensical_serve::http::{Request, Response, Status};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and convert into handler
    /// let router = Router::default()
    ///     .get("/coffee", |req: Request, params: Params| {
    ///         Response::new().status(Status::ImATeapot)
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

// ----------------------------------------------------------------------------

impl Default for Router {
    /// Creates a default router.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::router::Router;
    ///
    /// // Create router
    /// let router = Router::default();
    /// ```
    fn default() -> Self {
        Self {
            builders: Vec::default(),
            path: String::from("/"),
        }
    }
}
