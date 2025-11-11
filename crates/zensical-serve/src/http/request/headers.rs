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

//! HTTP request headers.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use crate::http::Header;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP request headers.
///
/// The header map for HTTP requests can store borrowed and owned values, which
/// allows for zero-copy parsing of headers, since the [`Request`][] is already
/// borrowed. Using a [`Cow`] allows middlewares to alter the headers, limiting
/// allocations to the case where headers are added or modified.
///
/// As keys are integers, it's better to use a [`BTreeMap`] than a [`HashMap`],
/// because the latter is 3x slower for integer keys.
///
/// [`HashMap`]: std::collections::HashMap
/// [`Request`]: crate::http::Request
///
/// # Examples
///
/// ```
/// use zensical_serve::http::request::Headers;
/// use zensical_serve::http::Header;
///
/// // Create header map and add header
/// let mut headers = Headers::new();
/// headers.insert(Header::Accept, "text/plain");
///
/// // Obtain string representation
/// println!("{headers}");
/// ```
#[derive(Clone, Debug, Default)]
pub struct Headers<'a> {
    /// Ordered map of headers.
    inner: BTreeMap<Header, Cow<'a, str>>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Headers<'a> {
    /// Creates a header map.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::request::Headers;
    ///
    /// // Create header map
    /// let headers = Headers::new();
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self { inner: BTreeMap::new() }
    }

    /// Returns the value for the given header.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::request::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::Accept, "text/plain");
    ///
    /// // Obtain reference to header value
    /// let value = headers.get(Header::Accept);
    /// ```
    #[inline]
    #[must_use]
    pub fn get(&self, header: Header) -> Option<&str> {
        self.inner.get(&header).map(AsRef::as_ref)
    }

    /// Returns whether the header is contained.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::request::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::Accept, "text/plain");
    ///
    /// // Ensure presence of header
    /// let check = headers.contains(Header::Accept);
    /// assert_eq!(check, true);
    /// ```
    #[inline]
    #[must_use]
    pub fn contains(&self, header: Header) -> bool {
        self.inner.contains_key(&header)
    }

    /// Updates the given header.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::request::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::Accept, "text/plain");
    /// ```
    #[inline]
    pub fn insert<V>(&mut self, header: Header, value: V)
    where
        V: Into<Cow<'a, str>>,
    {
        self.inner.insert(header, value.into());
    }

    /// Removes the given header.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::request::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::Accept, "text/plain");
    ///
    /// // Remove header
    /// headers.remove(Header::Accept);
    /// ```
    #[inline]
    pub fn remove(&mut self, header: Header) {
        self.inner.remove(&header);
    }
}

#[allow(clippy::must_use_candidate)]
impl Headers<'_> {
    /// Returns the number of headers.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether there are any headers.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<'a> FromIterator<(Header, &'a str)> for Headers<'a> {
    /// Creates a header map from an iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::request::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map from iterator
    /// let headers = Headers::from_iter([
    ///     (Header::Accept, "text/plain"),
    ///     (Header::AcceptLanguage, "en"),
    /// ]);
    /// ```
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (Header, &'a str)>,
    {
        let mut headers = Headers::new();
        for (header, value) in iter {
            headers.insert(header, value);
        }
        headers
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Headers<'_> {
    /// Formats the header map for display.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (header, value) in &self.inner {
            f.write_str(header.name())?;
            f.write_str(": ")?;
            f.write_str(value)?;
            f.write_str("\r\n")?;
        }

        // No errors occurred
        Ok(())
    }
}
