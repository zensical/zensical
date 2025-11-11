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

//! Matcher route.

use std::fmt;
use std::str::FromStr;

mod error;

pub use error::{Error, Result};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Matcher route.
///
/// Routes are just non-empty strings that have been confirmed to start with `/`
/// and not end with `/`, which makes joining them significantly easier. Routes
/// might contain parameters, which are denoted by `{...}` brackets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// Route path.
    path: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Route {
    /// Appends the given route to the route.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use std::str::FromStr;
    /// use zensical_serve::handler::matcher::Route;
    ///
    /// // Create route
    /// let route = Route::from_str("/coffee")?;
    ///
    /// // Append another route
    /// let route = route.append("/{kind}".parse()?);
    /// assert_eq!(route.to_string(), "/coffee/{kind}");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn append(&self, route: Self) -> Self {
        if self.path == "/" {
            route
        } else if route.path == "/" {
            self.clone()
        } else {
            // Compute the size of the new route path
            let capacity = self.path.len() + route.path.len();
            let mut path = String::with_capacity(capacity);

            // Concatenate the two route paths
            path.push_str(self.path.as_str());
            path.push_str(route.path.as_str());
            Self { path }
        }
    }
}

#[allow(clippy::must_use_candidate)]
impl Route {
    /// Returns the string representation.
    #[inline]
    pub fn as_str(&self) -> &str {
        self.path.as_str()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl FromStr for Route {
    type Err = Error;

    /// Attempts to create a route from a string.
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
    /// use zensical_serve::handler::matcher::Route;
    ///
    /// // Create route from string
    /// let route: Route = "/coffee/{kind}".parse()?;
    /// # Ok(())
    /// # }
    /// ```
    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Err(Error::Empty);
        }

        // Ensure route starts with `/`
        if !value.starts_with('/') {
            return Err(Error::Relative(value.to_string()));
        }

        // Ensure route doesn't end with `/`
        if value.len() > 1 && value.ends_with('/') {
            return Err(Error::Trailing(value.to_string()));
        }

        // No errors occurred
        Ok(Self { path: value.to_string() })
    }
}

// ----------------------------------------------------------------------------

impl AsRef<str> for Route {
    /// Returns the string representation.
    fn as_ref(&self) -> &str {
        self.path.as_str()
    }
}

// ----------------------------------------------------------------------------

impl Default for Route {
    /// Creates a default route.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::handler::matcher::Route;
    ///
    /// // Create route
    /// let route = Route::default();
    /// assert_eq!(route.as_str(), "/");
    /// ```
    fn default() -> Self {
        Self { path: String::from("/") }
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Route {
    /// Formats the route for display.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.path)
    }
}
