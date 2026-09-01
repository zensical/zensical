// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

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

use pyo3::exceptions::PyTypeError;
use pyo3::types::{
    PyAny, PyAnyMethods, PyBool, PyDict, PyFloat, PyInt, PyList, PyString,
};
use pyo3::{Borrowed, FromPyObject, PyErr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

mod float;

use float::Float;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Dynamic value.
///
/// This data type represents any valid value that can be used as part of the
/// metadata of a page and the extra data of configuration, supporting strings,
/// nulls, booleans, integers, floating point numbers, lists, and maps, so
/// basically everything supported in YAML and TOML.
///
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dynamic {
    /// Null value.
    Null,
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
// Implementations
// ----------------------------------------------------------------------------

impl Dynamic {
    /// Creates a dynamic floating-point value.
    pub fn from_float(value: f64) -> Self {
        Self::Float(Float(value))
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl fmt::Display for Dynamic {
    /// Formats the dynamic value for display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dynamic::Null => write!(f, "null"),
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

// ----------------------------------------------------------------------------

impl<'a, 'py> FromPyObject<'a, 'py> for Dynamic {
    type Error = PyErr;

    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> Result<Self, Self::Error> {
        if obj.is_none() {
            Ok(Self::Null)
        } else if obj.is_instance_of::<PyBool>() {
            obj.extract().map(Self::Bool)
        } else if obj.is_instance_of::<PyInt>() {
            obj.extract().map(Self::Integer)
        } else if obj.is_instance_of::<PyFloat>() {
            obj.extract().map(|value| Self::Float(Float(value)))
        } else if obj.is_instance_of::<PyString>() {
            obj.extract().map(Self::String)
        } else if obj.is_instance_of::<PyList>() {
            obj.extract().map(Self::List)
        } else if obj.is_instance_of::<PyDict>() {
            obj.extract().map(Self::Map)
        } else {
            Err(PyTypeError::new_err("unsupported dynamic value"))
        }
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::Dynamic;

    #[test]
    fn null_round_trips_through_json() {
        let data = serde_json::to_string(&Dynamic::Null).unwrap();
        assert_eq!(data, "null");
        assert_eq!(
            serde_json::from_str::<Dynamic>(&data).unwrap(),
            Dynamic::Null
        );
    }
}
