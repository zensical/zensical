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

//! Shared representation for typed logical paths.

use std::path::{Component, Path};
use std::sync::Arc;

use super::PathError;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Canonical platform-independent relative path.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RelativePath(Arc<str>);

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl RelativePath {
    /// Parses a canonical `/`-separated logical path.
    pub fn parse(path: &str) -> Result<Self, PathError> {
        if path.is_empty() {
            return Err(PathError::Empty);
        }
        if path.contains('\0') {
            return Err(PathError::Nul);
        }
        if path.contains('\\') {
            return Err(PathError::Backslash(path.into()));
        }
        if path.starts_with('/') || has_windows_prefix(path) {
            return Err(PathError::Absolute(path.into()));
        }
        for component in path.split('/') {
            match component {
                "" | "." => {
                    return Err(PathError::NonCanonical(path.into()));
                }
                ".." => return Err(PathError::Parent(path.into())),
                _ => {}
            }
        }
        Ok(Self(Arc::from(path)))
    }

    /// Converts a relative native path without lossy string conversion.
    pub fn from_path(path: &Path) -> Result<Self, PathError> {
        if path.is_absolute() {
            return Err(PathError::Absolute(path.display().to_string()));
        }
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(component) => components.push(
                    component.to_str().ok_or(PathError::NonUtf8)?.to_owned(),
                ),
                Component::CurDir => {
                    return Err(PathError::NonCanonical(
                        path.display().to_string(),
                    ));
                }
                Component::ParentDir => {
                    return Err(PathError::Parent(path.display().to_string()));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(PathError::Absolute(
                        path.display().to_string(),
                    ));
                }
            }
        }
        Self::parse(&components.join("/"))
    }

    /// Returns the canonical string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Iterates over path components.
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.0.split('/')
    }

    /// Returns the final component.
    pub fn file_name(&self) -> &str {
        self.components().next_back().expect("nonempty path")
    }

    /// Returns the final component's extension.
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        name.rsplit_once('.')
            .filter(|(stem, _)| !stem.is_empty())
            .map(|(_, extension)| extension)
    }

    /// Returns the final component without its extension.
    pub fn file_stem(&self) -> &str {
        let name = self.file_name();
        name.rsplit_once('.')
            .filter(|(stem, _)| !stem.is_empty())
            .map_or(name, |(stem, _)| stem)
    }

    /// Returns the number of components.
    pub fn depth(&self) -> usize {
        self.components().count()
    }

    /// Returns the parent path, if this path has more than one component.
    pub fn parent(&self) -> Option<Self> {
        self.0
            .rfind('/')
            .map(|index| Self(Arc::from(&self.0[..index])))
    }

    /// Returns whether this path is a strict component descendant of `base`.
    pub fn is_descendant_of(&self, base: &Self) -> bool {
        self.0
            .strip_prefix(base.as_str())
            .is_some_and(|suffix| suffix.starts_with('/'))
    }

    /// Appends one or more canonical relative components.
    pub fn join(&self, path: &str) -> Result<Self, PathError> {
        let path = Self::parse(path)?;
        Self::parse(&format!("{}/{}", self.as_str(), path.as_str()))
    }

    /// Replaces the final component with one canonical file name.
    pub fn with_file_name(&self, name: &str) -> Result<Self, PathError> {
        let name = Self::parse(name)?;
        if name.depth() != 1 {
            return Err(PathError::NonCanonical(name.as_str().into()));
        }
        match self.parent() {
            Some(parent) => parent.join(name.as_str()),
            None => Ok(name),
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Detects Windows drive-relative and drive-absolute prefixes on every host.
fn has_windows_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{PathError, RelativePath};

    #[test]
    fn accepts_canonical_unicode_and_percent_paths() {
        for path in [
            "index.md",
            "guide/page.md",
            "café/Überblick.md",
            "100%/page.md",
            ".meta.yml",
        ] {
            assert_eq!(RelativePath::parse(path).unwrap().as_str(), path);
        }
    }

    #[test]
    fn rejects_noncanonical_and_unsafe_spellings() {
        let cases = [
            "",
            "/page.md",
            "./page.md",
            "guide/./page.md",
            "guide//page.md",
            "guide/page.md/",
            "../page.md",
            "guide/../page.md",
            "guide\\page.md",
            "C:page.md",
            "C:/page.md",
            "page\0.md",
        ];
        for path in cases {
            assert!(RelativePath::parse(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn component_operations_are_lexical_and_platform_independent() {
        let path = RelativePath::parse("guide/nested/page.md").unwrap();
        assert_eq!(path.file_name(), "page.md");
        assert_eq!(path.extension(), Some("md"));
        assert_eq!(path.file_stem(), "page");
        assert_eq!(path.depth(), 3);
        assert_eq!(path.parent().unwrap().as_str(), "guide/nested");
        assert!(path.is_descendant_of(&RelativePath::parse("guide").unwrap()));
        assert!(!RelativePath::parse("guidelines/page.md")
            .unwrap()
            .is_descendant_of(&RelativePath::parse("guide").unwrap()));
        assert_eq!(
            RelativePath::parse("guide")
                .unwrap()
                .join("nested/page.md")
                .unwrap()
                .as_str(),
            path.as_str()
        );
        assert_eq!(
            path.with_file_name("other.html").unwrap().as_str(),
            "guide/nested/other.html"
        );
        assert!(path.with_file_name("other/name.html").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_native_components() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;
        use std::path::PathBuf;

        let path = PathBuf::from(OsString::from_vec(b"bad-\xff.md".to_vec()));
        assert_eq!(RelativePath::from_path(&path), Err(PathError::NonUtf8));
    }
}
