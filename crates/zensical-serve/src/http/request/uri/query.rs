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

//! HTTP query string.

use std::borrow::Cow;
use std::{fmt, iter, str};

mod encoding;

use encoding::{decode, encode};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP query string.
///
/// As with the other components that can be part of a [`Request`][], the query
/// string can store borrowed and owned values, which allows for very efficient
/// handling, while still being able to comfortably overwrite parameters of
/// query strings in middlewares and tests, if necessary.
///
/// When parsing a query string with [`Query::from`], the keys and values will
/// be percent-decoded and stored decoded in a parameter list, as query strings
/// might have multiple values for the same key, and ordering always needs to
/// be preserved when formatting with [`fmt::Display`]. Note that only those
/// characters for which percent-encoding is required will be percent-encoded
/// when printing the query string.
///
/// [`Request`]: crate::http::Request
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query<'a> {
    /// List of parameters.
    inner: Vec<Param<'a>>,
}

/// HTTP query string parameter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Param<'a> {
    /// Parameter key.
    key: Cow<'a, str>,
    /// Parameter value.
    value: Cow<'a, str>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Query<'a> {
    /// Creates a query string.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string
    /// let query = Query::new();
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the first parameter value for the given key.
    ///
    /// If the parameter appears multiple times in the query string, only the
    /// first value is returned. Use [`Query::get_all`] to retrieve all values.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string and add parameter
    /// let mut query = Query::new();
    /// query.add("key", "value");
    ///
    /// // Obtain reference to parameter value
    /// let value = query.get("key");
    /// ```
    pub fn get<K>(&self, key: K) -> Option<&str>
    where
        K: AsRef<str>,
    {
        self.inner.iter().find_map(|param| {
            (param.key == key.as_ref()).then_some(param.value.as_ref())
        })
    }

    /// Returns an iterator over all parameter values for the given key.
    ///
    /// This is particularly necessary for query string parameters which can be
    /// repeated, such as form submissions with multiple checkboxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string and add parameters
    /// let mut query = Query::new();
    /// query.add("key", "a");
    /// query.add("key", "b");
    ///
    /// // Iterate over parameter values
    /// for value in query.get_all("key") {
    ///     println!("{value}");
    /// }
    /// ```
    pub fn get_all<K>(&self, key: K) -> impl Iterator<Item = &str>
    where
        K: AsRef<str>,
    {
        self.inner.iter().filter_map(move |param| {
            (param.key == key.as_ref()).then_some(param.value.as_ref())
        })
    }

    /// Returns whether the parameter is contained.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string and add parameter
    /// let mut query = Query::new();
    /// query.add("key", "value");
    ///
    /// // Ensure presence of parameter
    /// let check = query.contains("key");
    /// assert_eq!(check, true);
    /// ```
    pub fn contains<K>(&self, key: K) -> bool
    where
        K: AsRef<str>,
    {
        self.inner.iter().any(|param| param.key == key.as_ref())
    }

    /// Adds the given key-value pair as a parameter.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string and add parameter
    /// let mut query = Query::new();
    /// query.add("key", "value");
    /// ```
    pub fn add<K, V>(&mut self, key: K, value: V)
    where
        K: Into<Cow<'a, str>>,
        V: Into<Cow<'a, str>>,
    {
        self.inner.push(Param {
            key: key.into(),
            value: value.into(),
        });
    }

    /// Removes the given parameter.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string and add parameter
    /// let mut query = Query::new();
    /// query.add("key", "value");
    ///
    /// // Remove parameter
    /// query.remove("key");
    /// ```
    pub fn remove<K>(&mut self, key: K)
    where
        K: AsRef<str>,
    {
        self.inner.retain(|param| param.key != key.as_ref());
    }
}

#[allow(clippy::must_use_candidate)]
impl Query<'_> {
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

impl<'a> From<&'a str> for Query<'a> {
    /// Creates a query string from a string.
    ///
    /// The query string is parsed from the given string, which is expected to
    /// be in the format of a query string, i.e., a sequence of key-value pairs
    /// connected with `&`, but with the initial `?` separator removed. Both
    /// keys and values are percent-decoded and stored.
    ///
    /// Note that we can't implement [`FromStr`][] for [`Query`] because of the
    /// required `&'a str` lifetime, which is not compatible with the trait.
    ///
    /// [`FromStr`]: std::str::FromStr
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string from string
    /// let query = Query::from("query=search&limit=25");
    /// ```
    #[allow(clippy::missing_panics_doc)]
    fn from(value: &'a str) -> Self {
        let mut pairs = Vec::new();

        // Initialize start and pair index
        let mut start = 0;
        let mut index = 0;

        // Extract key-value pairs from string after conversion - we append a
        // sentinel `&` separator to the end of the string, which makes parsing
        // much simpler, as we don't need to replicate the logic for the last
        // key-value pair outside of the loop
        let chars = value.char_indices();
        for (i, char) in chars.chain(iter::once((value.len(), '&'))) {
            match char {
                // If the current character is a `=` separator, we consumed a
                // key (which may be empty), so we start a new key-value pair.
                // Note that the `=` separator can also appear multiple times,
                // in which case it's treated as a verbatim character.
                '=' if index == pairs.len() => {
                    pairs.push((decode(&value[start..i]), Cow::Borrowed("")));
                    start = i + 1;
                }

                // If the current character is a `&` separator, we consumed a
                // key-value pair, or just a key, both of which might be empty
                '&' if start != i.saturating_sub(1) => {
                    if index < pairs.len() && pairs[index].1.is_empty() {
                        pairs[index].1 = decode(&value[start..i]);
                    } else {
                        pairs.push((
                            decode(&value[start..i]),
                            Cow::Borrowed(""),
                        ));
                    }

                    // Continue after separator
                    start = i + 1;
                    index += 1;
                }

                // Consume all other characters
                _ => {}
            }
        }

        // Create query string from key-value pairs
        Query::from_iter(pairs)
    }
}

// ----------------------------------------------------------------------------

impl<'a, K, V> FromIterator<(K, V)> for Query<'a>
where
    K: Into<Cow<'a, str>>,
    V: Into<Cow<'a, str>>,
{
    /// Creates a query string from an iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::http::Query;
    ///
    /// // Create query string from iterator
    /// let query = Query::from_iter([
    ///     ("query", "search"),
    ///     ("limit", "25"),
    /// ]);
    /// ```
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (K, V)>,
    {
        let mut query = Query::new();
        for (key, value) in iter {
            query.add(key, value);
        }
        query
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Query<'_> {
    /// Formats the query string for display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, param) in self.inner.iter().enumerate() {
            if i > 0 {
                f.write_str("&")?;
            }

            // Write parameter key and value, if any
            f.write_str(encode(&param.key).as_ref())?;
            if !param.value.is_empty() {
                f.write_str("=")?;
                f.write_str(encode(&param.value).as_ref())?;
            }
        }

        // No errors occurred
        Ok(())
    }
}
