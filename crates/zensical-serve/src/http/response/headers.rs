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

//! HTTP response headers.

use std::collections::btree_map::Iter;
use std::collections::BTreeMap;
use std::fmt;

use crate::http::Header;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP response headers.
///
/// The header map for HTTP responses stores owned values, so we don't need to
/// bother the middleware signatures with lifetimes, which would make writing
/// middlewares much more complicated. Here, we prefer a simple interface over
/// one that optimizes for performance.
///
/// As keys are integers, it's better to use a [`BTreeMap`] than a [`HashMap`],
/// because the latter is 3x slower for integer keys.
///
/// [`HashMap`]: std::collections::HashMap
///
/// # Examples
///
/// ```
/// use zensical_serve::http::response::Headers;
/// use zensical_serve::http::Header;
///
/// // Create header map and add header
/// let mut headers = Headers::new();
/// headers.insert(Header::ContentType, "text/plain");
///
/// // Obtain string representation
/// println!("{headers}");
/// ```
#[derive(Clone, Debug, Default)]
pub struct Headers {
    /// Ordered map of headers.
    inner: BTreeMap<Header, String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Headers {
    /// Creates a header map.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::response::Headers;
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
    /// use zensical_serve::http::response::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::ContentType, "text/plain");
    ///
    /// // Obtain reference to header value
    /// let value = headers.get(Header::ContentType);
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
    /// use zensical_serve::http::response::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::ContentType, "text/plain");
    ///
    /// // Ensure presence of header
    /// let check = headers.contains(Header::ContentType);
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
    /// use zensical_serve::http::response::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::ContentType, "text/plain");
    /// ```
    #[allow(clippy::needless_pass_by_value)]
    #[inline]
    pub fn insert<V>(&mut self, header: Header, value: V)
    where
        V: ToString,
    {
        self.inner.insert(header, value.to_string());
    }

    /// Removes the given header.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::response::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::ContentType, "text/plain");
    ///
    /// // Remove header
    /// headers.remove(Header::ContentType);
    /// ```
    #[inline]
    pub fn remove(&mut self, header: Header) {
        self.inner.remove(&header);
    }

    /// Returns an iterator over the header map.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::response::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::ContentType, "text/plain");
    ///
    /// // Iterate over header map
    /// for (header, value) in headers.iter() {
    ///    println!("{header}: {value}");
    /// }
    /// ```
    #[inline]
    pub fn iter(&self) -> Iter<'_, Header, String> {
        self.inner.iter()
    }
}

#[allow(clippy::must_use_candidate)]
impl Headers {
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

impl<'a> IntoIterator for &'a Headers {
    type Item = (&'a Header, &'a String);
    type IntoIter = Iter<'a, Header, String>;

    /// Creates an iterator over the header map.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::response::Headers;
    /// use zensical_serve::http::Header;
    ///
    /// // Create header map and add header
    /// let mut headers = Headers::new();
    /// headers.insert(Header::ContentType, "text/plain");
    ///
    /// // Iterate over header map
    /// for (header, value) in &headers {
    ///    println!("{header}: {value}");
    /// }
    /// ```
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Headers {
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
