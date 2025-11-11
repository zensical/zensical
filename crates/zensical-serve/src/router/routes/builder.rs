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

//! Routes builder.

use std::collections::BTreeMap;
use std::str::FromStr;

use crate::handler::{Error, Matcher, Result, Scope};
use crate::http::Method;
use crate::middleware::TryIntoMiddleware;
use crate::router::{Action, Route};

use super::Routes;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Routes builder.
#[allow(clippy::type_complexity)]
#[derive(Debug)]
pub struct Builder {
    /// Map methods to routes.
    routes: BTreeMap<Method, Vec<(String, Box<dyn Action>)>>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Builder {
    /// Creates a routes builder.
    #[allow(clippy::new_without_default)]
    #[must_use]
    pub fn new() -> Self {
        Self { routes: BTreeMap::new() }
    }

    /// Adds a route to the routes.
    ///
    /// Note that this method is infallible, as routes are converted into paths
    /// when building the matchers, not when they're added. This is particularly
    /// necessary for the [`matchit`] crate, which requires all routes to be
    /// unique, but also allows for a more streamlined API.
    pub fn add<P, A>(&mut self, method: Method, path: P, action: A)
    where
        P: Into<String>,
        A: Action,
    {
        self.routes
            .entry(method)
            .or_default()
            .push((path.into(), Box::new(action)));
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl TryIntoMiddleware for Builder {
    type Output = Routes;

    /// Attempts to convert the routes into a middleware.
    fn try_into_middleware(self, scope: &Scope) -> Result<Self::Output> {
        // Obtain the matcher's base path from the given scope, and prepend it
        // to all routes, allowing for the creation of nested routers
        let base = match scope.route.as_ref() {
            Some(route) => route,
            None => &Route::default(),
        };

        // Transform all registered routes into a single router for each method,
        // after checking whether each route is valid and does not overlap with
        // any other route. Note that non-overlap is checked by the third-party
        // router, which is a requirement for its matching algorithm.
        let iter = self.routes.into_iter().map(|(method, items)| {
            let mut matcher = Matcher::new();
            for (path, action) in items {
                let path = Route::from_str(&path)
                    .map_err(|err| Error::Matcher(err.into()))?;

                // Join the matcher's base path with the route path and add it
                // to the matcher, associating it with the registered action
                matcher.add(base.append(path), action)?;
            }
            Ok((method, matcher))
        });

        // Collect methods and routes into an ordered map
        iter.collect::<Result<BTreeMap<_, _>>>()
            .map(|routes| Routes { matchers: routes })
    }
}
