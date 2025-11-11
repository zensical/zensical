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

//! Matcher parameters.

use matchit::ParamsIter;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Matcher parameters.
///
/// This is a thin wrapper around the [`Params`][] data type of the [`matchit`]
/// crate to shield against unforeseen changes in the crate's implementation.
///
/// Note that [`Params`] should be used without lifetime parameters, as it's
/// designed to be used in a context where lifetimes are irrelevant, i.e., in
/// the signature of an [`Action`][], ensuring that third-party integrations
/// don't break when the implementation changes, albeit unlikely.
///
/// [`Action`]: crate::router::Action
/// [`Params`]: matchit::Params
#[derive(Clone, Debug)]
pub struct Params<'k, 'v> {
    /// Parameter list implementation.
    inner: matchit::Params<'k, 'v>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'k, 'v> Params<'k, 'v> {
    /// Creates matcher parameters.
    ///
    /// This method is used by the [`Matcher`][] to create matcher parameters
    /// from the [`matchit::Params`] as returned by [`matchit`].
    ///
    /// [`Matcher`]: crate::handler::Matcher
    #[inline]
    pub(crate) fn new(inner: matchit::Params<'k, 'v>) -> Self {
        Params { inner }
    }
}

impl<'k, 'v> Params<'k, 'v> {
    /// Returns the value for the given key.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response, Status};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .get("/coffee/{kind}", |req: Request, params: Params| {
    ///         if let Some(kind) = params.get("kind") {
    ///             Response::default()
    ///         } else {
    ///             Response::new().status(Status::BadRequest)
    ///         }
    ///     });
    /// ```
    #[inline]
    pub fn get<K>(&self, key: K) -> Option<&'v str>
    where
        K: AsRef<str>,
    {
        self.inner.get(key)
    }

    /// Returns whether the parameter is contained.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response, Status};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .get("/coffee/{kind}", |req: Request, params: Params| {
    ///         if params.contains("kind") {
    ///             Response::default()
    ///         } else {
    ///             Response::new().status(Status::BadRequest)
    ///         }
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn contains<K>(&self, key: K) -> bool
    where
        K: AsRef<str>,
    {
        self.inner.get(key).is_some()
    }

    /// Returns an iterator over all parameters.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .get("/coffee/{kind}", |req: Request, params: Params| {
    ///         for (key, value) in params.iter() {
    ///             println!("{key}: {value}");
    ///         }
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    #[must_use]
    pub fn iter(&self) -> ParamsIter<'_, 'k, 'v> {
        self.inner.iter()
    }
}

#[allow(clippy::must_use_candidate)]
impl Params<'_, '_> {
    /// Returns the number of parameters.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether there are any parameters.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<'a, 'k, 'v> IntoIterator for &'a Params<'k, 'v> {
    type Item = (&'k str, &'v str);
    type IntoIter = ParamsIter<'a, 'k, 'v>;

    /// Creates an iterator over all parameters.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::{Request, Response};
    /// use zensical_serve::router::{Router, Params};
    ///
    /// // Create router and add route
    /// let router = Router::default()
    ///     .get("/coffee/{kind}", |req: Request, params: Params| {
    ///         for (key, value) in &params {
    ///             println!("{key}: {value}");
    ///         }
    ///         Response::default()
    ///     });
    /// ```
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
