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

//! Site-relative output paths.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use super::relative::RelativePath;
use super::PathError;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Canonical path relative to the site output root.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SitePath(RelativePath);

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl SitePath {
    /// Converts a relative native path without lossy string conversion.
    pub fn from_path(path: &Path) -> Result<Self, PathError> {
        RelativePath::from_path(path).map(Self)
    }

    /// Returns the canonical string representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Iterates over site path components.
    #[must_use]
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.0.components()
    }

    /// Returns the output file name.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.file_name()
    }

    /// Returns the output file extension.
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.0.extension()
    }

    /// Returns the output file name without its extension.
    #[must_use]
    pub fn file_stem(&self) -> &str {
        self.0.file_stem()
    }

    /// Returns the number of path components.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.depth()
    }

    /// Returns the parent site path.
    pub fn parent(&self) -> Option<Self> {
        self.0.parent().map(Self)
    }

    /// Returns whether this output is a strict descendant of `base`.
    #[must_use]
    pub fn is_descendant_of(&self, base: &Self) -> bool {
        self.0.is_descendant_of(&base.0)
    }

    /// Appends a canonical site-relative path.
    pub fn join(&self, path: &str) -> Result<Self, PathError> {
        self.0.join(path).map(Self)
    }

    /// Replaces the output file name.
    pub fn with_file_name(&self, name: &str) -> Result<Self, PathError> {
        self.0.with_file_name(name).map(Self)
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl FromStr for SitePath {
    type Err = PathError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        RelativePath::parse(path).map(Self)
    }
}

impl AsRef<str> for SitePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SitePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for SitePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SitePath {
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
    use super::SitePath;

    #[test]
    fn retains_a_distinct_site_path_domain() {
        let path = "assets/app.min.js".parse::<SitePath>().unwrap();
        assert_eq!(path.file_name(), "app.min.js");
        assert_eq!(path.extension(), Some("js"));
        assert_eq!(path.file_stem(), "app.min");
        assert_eq!(path.parent().unwrap().as_str(), "assets");
        assert_eq!(
            path.with_file_name("vendor.js").unwrap().as_str(),
            "assets/vendor.js"
        );
    }
}
