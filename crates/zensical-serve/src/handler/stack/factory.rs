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

//! Factory.

use std::fmt;

use crate::handler::{Result, Scope};
use crate::middleware::Middleware;

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

/// Factory.
///
/// While stacks and middlewares are naturally composed bottom-up due to the
/// builder-like nature of their interface, factories allow us to turn this on
/// its head, and create middlewares top-down, which ensures that the [`Scope`]
/// can efficiently and correctly propagate through nested stacks.
///
/// Factories are like type-erased implementations of [`TryIntoMiddleware`][],
/// and return boxed implementations of [`Middleware`][], and are essentially
/// and implementation detail of the [`Stack`][], Implementors should always
/// implement [`TryIntoMiddleware`][].
///
/// [`TryIntoMiddleware`]: crate::middleware::TryIntoMiddleware
/// [`Stack`]: crate::handler::Stack
pub trait Factory: FnOnce(&Scope) -> Result<Box<dyn Middleware>> {}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl fmt::Debug for Box<dyn Factory> {
    /// Formats the factory for debugging.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Box<dyn Factory>")
    }
}

// ----------------------------------------------------------------------------
// Blanket implementations
// ----------------------------------------------------------------------------

#[rustfmt::skip]
impl<F> Factory for F
where
    F: FnOnce(&Scope) -> Result<Box<dyn Middleware>> {}
