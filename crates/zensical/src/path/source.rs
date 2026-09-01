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

//! Provider-relative source paths.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use super::relative::RelativePath;
use super::PathError;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Canonical path relative to one provider source root.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourcePath(RelativePath);

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl SourcePath {
    /// Converts a relative native path without lossy string conversion.
    pub fn from_path(path: &Path) -> Result<Self, PathError> {
        RelativePath::from_path(path).map(Self)
    }

    /// Returns the canonical string representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Iterates over source path components.
    #[must_use]
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.0.components()
    }

    /// Returns the source file name.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.file_name()
    }

    /// Returns the source file extension.
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.0.extension()
    }

    /// Returns the source file name without its extension.
    #[must_use]
    pub fn file_stem(&self) -> &str {
        self.0.file_stem()
    }

    /// Returns the number of path components.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.depth()
    }

    /// Returns the parent source path.
    pub fn parent(&self) -> Option<Self> {
        self.0.parent().map(Self)
    }

    /// Returns whether this source is a strict descendant of `base`.
    #[must_use]
    pub fn is_descendant_of(&self, base: &Self) -> bool {
        self.0.is_descendant_of(&base.0)
    }

    /// Appends a canonical source-relative path.
    pub fn join(&self, path: &str) -> Result<Self, PathError> {
        self.0.join(path).map(Self)
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl FromStr for SourcePath {
    type Err = PathError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        RelativePath::parse(path).map(Self)
    }
}

impl AsRef<str> for SourcePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SourcePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for SourcePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SourcePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = String::deserialize(deserializer)?;
        path.parse().map_err(serde::de::Error::custom)
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::SourcePath;

    #[test]
    fn serializes_as_a_validated_string() {
        let path = "guide/café.md".parse::<SourcePath>().unwrap();
        assert_eq!(path.file_stem(), "café");
        let data = serde_json::to_string(&path).unwrap();
        assert_eq!(data, "\"guide/café.md\"");
        assert_eq!(serde_json::from_str::<SourcePath>(&data).unwrap(), path);
        assert!(serde_json::from_str::<SourcePath>("\"../page.md\"").is_err());
    }
}
