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

//! Matcher.

use std::str::FromStr;

mod error;
mod params;
mod route;

pub use error::{Error, Result};
pub use params::Params;
pub use route::Route;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Matcher.
///
/// This is a thin wrapper around the [`Router`][] data type of the [`matchit`]
/// crate to shield against unforeseen changes in the crate's implementation.
///
/// [`Router`]: matchit::Router
#[derive(Debug, Default)]
pub struct Matcher<T = ()> {
    /// Matcher implementation.
    inner: matchit::Router<T>,
}

/// Match.
#[derive(Debug)]
pub struct Match<'k, 'v, T = ()> {
    /// Match parameters.
    pub params: Params<'k, 'v>,
    /// Associated data.
    pub data: T,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<T> Matcher<T> {
    /// Creates a matcher.
    ///
    /// ```
    /// use zensical_serve::handler::Matcher;
    ///
    /// // Create matcher
    /// let matcher = Matcher::<()>::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self { inner: matchit::Router::new() }
    }

    /// Adds a route to the matcher.
    ///
    /// # Errors
    ///
    /// This method returns [`Error::Insert`], if the route could not be added
    /// to the matcher, including the reason for the failure.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use std::str::FromStr;
    /// use zensical_serve::handler::matcher::Route;
    /// use zensical_serve::handler::Matcher;
    ///
    /// // Create matcher and add route
    /// let mut matcher = Matcher::new();
    /// matcher.add(Route::from_str("/coffee/{kind}")?, ())?;
    /// # Ok(())
    /// # }
    /// ```
    #[allow(clippy::needless_pass_by_value)]
    pub fn add(&mut self, route: Route, value: T) -> Result {
        self.inner
            .insert(route.to_string(), value)
            .map_err(Into::into)
    }

    /// Attempts to resolve and match the given path.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use std::str::FromStr;
    /// use zensical_serve::handler::matcher::Route;
    /// use zensical_serve::handler::Matcher;
    ///
    /// // Create matcher and add route
    /// let mut matcher = Matcher::new();
    /// matcher.add(Route::from_str("/coffee/{kind}")?, ())?;
    ///
    /// // Resolve route from path
    /// let route = matcher.resolve("/coffee/vietnamese");
    /// assert!(route.is_some());
    /// # Ok(())
    /// # }
    /// ```
    pub fn resolve<'v>(&self, path: &'v str) -> Option<Match<'_, 'v, &T>> {
        self.inner.at(path).ok().map(|route| Match {
            params: Params::new(route.params),
            data: route.value,
        })
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl FromStr for Matcher {
    type Err = Error;

    /// Attempts to create a matcher from a string.
    ///
    /// This method is a convenient shortcut for creating a [`Matcher`] from a
    /// single [`Route`], which can be used in middlewares for matching routes.
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
    /// use zensical_serve::handler::Matcher;
    ///
    /// // Create matcher from string
    /// let matcher: Matcher = "/coffee/{kind}".parse()?;
    /// # Ok(())
    /// # }
    /// ```
    fn from_str(value: &str) -> Result<Self> {
        let mut matcher = Self::new();
        matcher // fmt
            .add(Route::from_str(value)?, ())
            .map(|()| matcher)
    }
}
