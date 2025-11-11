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

//! Dynamic value.

use pyo3::FromPyObject;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

mod float;

use float::Float;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Dynamic value.
///
/// This data type represents any valid value that can be used as part of the
/// metadata of a page and the extra data of configuration, supporting strings,
/// booleans, integers, floating point numbers, lists, and maps, so basically
/// everything supported in YAML and TOML.
///
/// Null value are not supported, and currently represented as empty strings.
/// We're aiming to provide a type safe way to define custom namespaces in the
/// configuration, so we'll definitely revisit this as part of our efforts to
/// make configuration much more flexible.
#[derive(
    Clone, Debug, FromPyObject, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(untagged)]
#[pyo3(from_item_all)]
pub enum Dynamic {
    /// String value.
    String(String),
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Integer(i64),
    /// Floating point value.
    Float(Float),
    /// List value.
    List(Vec<Dynamic>),
    /// Map value.
    Map(BTreeMap<String, Dynamic>),
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl fmt::Display for Dynamic {
    /// Formats the dynamic value for display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dynamic::String(value) => write!(f, "{value}"),
            Dynamic::Bool(value) => write!(f, "{value}"),
            Dynamic::Integer(value) => write!(f, "{value}"),
            Dynamic::Float(value) => write!(f, "{value}"),
            Dynamic::List(values) => {
                let iter = values.iter().map(|v| format!("{v}"));
                let values: Vec<String> = iter.collect();
                write!(f, "[{}]", values.join(", "))
            }
            Dynamic::Map(values) => {
                let iter = values.iter().map(|(k, v)| format!("{k}: {v}"));
                let values: Vec<String> = iter.collect();
                write!(f, "{{{}}}", values.join(", "))
            }
        }
    }
}
