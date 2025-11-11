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

//! Floating point number with equality and hashing.

use pyo3::FromPyObject;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Floating point number.
#[derive(Clone, Debug, FromPyObject, Serialize, Deserialize)]
pub struct Float(pub f64);

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl PartialEq for Float {
    /// Compares two floating point numbers for equality.
    fn eq(&self, other: &Self) -> bool {
        (self.0 - other.0).abs() < f64::EPSILON
    }
}

impl Eq for Float {}

// ----------------------------------------------------------------------------

impl Hash for Float {
    /// Hashes the number.
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        state.write(&self.0.to_ne_bytes());
    }
}

impl fmt::Display for Float {
    /// Formats the floating point number.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
