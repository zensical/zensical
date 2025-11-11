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

//! HTTP request URI.

use std::borrow::Cow;
use std::fmt;

mod encoding;
mod query;

use encoding::{decode, encode};
pub use query::Query;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP request URI.
///
/// This is a lightweight, but definitely not spec-compliant, URI parser. The
/// sane thing would be to just use the [`url`][] crate, but it pulls in a huge
/// number of dependencies, which would double or triple the footprint of our
/// executable, and increase churn of dependencies for no immediate upside.
///
/// For now, we just assume that paths always start with a `/`, which is sane
/// to assume for a local web server that is not intended for proxying.
///
/// [`url`]: https://crates.io/crates/url
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uri<'a> {
    /// Request path.
    pub path: Cow<'a, str>,
    /// Query string.
    pub query: Query<'a>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Uri<'a> {
    /// Creates a request URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Uri;
    ///
    /// // Create request URI
    /// let uri = Uri::new();
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a request URI from a path and query string.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Uri;
    ///
    /// // Create request URI from parts
    /// let uri = Uri::from_parts("/path", "key=value");
    /// ```
    #[inline]
    #[must_use]
    pub fn from_parts<P, Q>(path: P, query: Q) -> Self
    where
        P: Into<Cow<'a, str>>,
        Q: Into<Query<'a>>,
    {
        Uri {
            path: path.into(),
            query: query.into(),
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<'a> From<&'a str> for Uri<'a> {
    /// Creates a request URI from a string.
    ///
    /// Note that we can't implement [`FromStr`][] for [`Uri`] because of the
    /// required `&'a str` lifetime, which is not compatible with the trait.
    ///
    /// [`FromStr`]: std::str::FromStr
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Uri;
    ///
    /// // Create request URI from string
    /// let uri = Uri::from("/path?key=value");
    /// ```
    fn from(value: &'a str) -> Self {
        match value.split_once('?') {
            Some((path, query)) => Uri {
                path: decode(path),
                query: Query::from(query),
            },
            None => Uri {
                path: decode(value),
                query: Query::default(),
            },
        }
    }
}

// ----------------------------------------------------------------------------

impl Default for Uri<'_> {
    /// Creates a default request URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Uri;
    ///
    /// // Create request URI
    /// let uri = Uri::default();
    #[inline]
    fn default() -> Self {
        Uri {
            path: Cow::Borrowed("/"),
            query: Query::default(),
        }
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Uri<'_> {
    /// Formats the request URI for display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(encode(&self.path).as_ref())?;

        // Write query string, if any
        if !self.query.is_empty() {
            f.write_str("?")?;
            self.query.fmt(f)?;
        }

        // No errors occurred
        Ok(())
    }
}
