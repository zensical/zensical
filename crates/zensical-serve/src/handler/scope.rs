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

//! Scope.

use super::matcher::Route;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Scope.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    // Base path for routes, optional.
    pub route: Option<Route>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Scope {
    /// Creates a scope.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::Scope;
    ///
    /// // Create scope
    /// let scope = Scope::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self { route: None }
    }

    /// Joins the scope with another scope.
    #[must_use]
    pub(crate) fn join<S>(&self, scope: S) -> Self
    where
        S: Into<Scope>,
    {
        let scope = scope.into();

        // If both scopes define a route, append the route of the given scope
        // to the route of the current scope. Otherwise, select the route.
        let route = match (self.route.as_ref(), scope.route) {
            (Some(head), Some(tail)) => Some(head.append(tail)),
            (Some(head), None) => Some(head.clone()),
            (None, Some(tail)) => Some(tail),
            (None, None) => None,
        };

        // Return scope
        Scope { route }
    }
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl From<Route> for Scope {
    /// Creates a scope from a route.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use std::str::FromStr;
    /// use zensical_serve::handler::matcher::Route;
    /// use zensical_serve::handler::Scope;
    ///
    /// // Create scope from route
    /// let route = Route::from_str("/coffee/{kind}")?;
    /// let scope = Scope::from(route);
    /// # Ok(())
    /// # }
    /// ```
    fn from(route: Route) -> Self {
        Scope { route: Some(route) }
    }
}
