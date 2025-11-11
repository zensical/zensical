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

//! HTTP method.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use super::error::{Error, Result};

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl AsRef<str> for Method {
    /// Returns the string representation.
    #[inline]
    fn as_ref(&self) -> &str {
        self.name()
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Method {
    /// Formats the method for display.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.name())
    }
}

// ----------------------------------------------------------------------------
// Macros
// ----------------------------------------------------------------------------

/// Defines and implements HTTP methods.
macro_rules! define_and_impl_method {
    (
        $(
            // Method definition
            $(#[$comment:meta])*
            $name:ident = $method:expr
        ),+
        $(,)?
    ) => {
        /// HTTP method.
        #[allow(dead_code)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Method {
            $(
                $(#[$comment])*
                $name,
            )+
        }

        impl Method {
            /// Returns the method name.
            ///
            /// # Examples
            ///
            /// ```
            /// use zensical_serve::http::Method;
            ///
            /// // Create method
            /// let method = Method::Get;
            ///
            /// // Obtain method name
            /// assert_eq!(method.name(), "GET");
            /// ```
            #[must_use]
            pub const fn name(&self) -> &'static str {
                match self {
                    $(
                        Method::$name => $method,
                    )+
                }
            }
        }

        /// Lookup table for HTTP methods (case-insensitive).
        static METHOD_LOOKUP_TABLE: LazyLock<HashMap<String, Method>> =
            LazyLock::new(|| {
                HashMap::from_iter([
                    $(
                        ($method.to_uppercase(), Method::$name),
                    )+
                ])
            });

        impl FromStr for Method {
            type Err = Error;

            /// Attempts to create a method from a string.
            ///
            /// # Errors
            ///
            /// This method returns [`Error::Method`], if the string does not
            /// match one of the known methods.
            ///
            /// # Examples
            ///
            /// ```
            /// # use std::error::Error;
            /// # fn main() -> Result<(), Box<dyn Error>> {
            /// use zensical_serve::http::Method;
            ///
            /// // Create method from string
            /// let method: Method = "GET".parse()?;
            /// # Ok(())
            /// # }
            /// ```
            fn from_str(value: &str) -> Result<Self> {
                METHOD_LOOKUP_TABLE
                    .get(&value.to_uppercase())
                    .copied()
                    .ok_or_else(|| Error::Method(value.to_string()))
            }
        }
    }
}

// ----------------------------------------------------------------------------

define_and_impl_method! {
    /// GET method
    Get = "GET",
    /// HEAD method
    Head = "HEAD",
    /// POST method
    Post = "POST",
    /// PUT method
    Put = "PUT",
    /// DELETE method
    Delete = "DELETE",
    /// OPTIONS method
    Options = "OPTIONS",
    /// TRACE method
    Trace = "TRACE",
    /// PATCH method
    Patch = "PATCH",
}
