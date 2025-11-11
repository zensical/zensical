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

//! Middleware.

use super::Middleware;
use crate::handler::{Result, Scope};

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

/// Attempt conversion into [`Middleware`].
pub trait TryIntoMiddleware: 'static {
    /// Output type of conversion.
    type Output: Middleware;

    /// Attempts to convert into a middleware.
    ///
    /// Since conversion can be fallible, it's a good idea to move validation
    /// prior to middleware instantiation into this method. This allows to keep
    /// the number of fallible methods as low as possible and allows for a more
    /// fluent API, as well as better error handling.
    ///
    /// Although middlewares are usually boxed, we return a concrete type, as
    /// it enables the compiler to employ monomorphization, if applicable.
    ///
    /// # Errors
    ///
    /// In case conversion fails, an error should be returned.
    fn try_into_middleware(self, scope: &Scope) -> Result<Self::Output>;
}

// ----------------------------------------------------------------------------
// Blanket implementations
// ----------------------------------------------------------------------------

impl<M> TryIntoMiddleware for M
where
    M: Middleware,
{
    type Output = Self;

    #[inline]
    fn try_into_middleware(self, _scope: &Scope) -> Result<Self> {
        Ok(self)
    }
}
