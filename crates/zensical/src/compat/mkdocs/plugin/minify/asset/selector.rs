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

//! Minify asset selector.

use anyhow::anyhow;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::collections::BTreeSet;

use crate::path::SitePath;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Compiled exact and glob asset selectors.
#[derive(Clone, Debug)]
pub struct Selector {
    exact: BTreeSet<String>,
    globs: GlobSet,
    error: Option<String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Selector {
    /// Compiles configured asset selectors.
    pub fn new(patterns: &[String]) -> Self {
        let mut exact = BTreeSet::new();
        let mut builder = GlobSetBuilder::new();
        let mut error = None;
        for pattern in patterns {
            let pattern = normalize(pattern);
            if let Err(reason) = pattern.parse::<SitePath>() {
                error.get_or_insert_with(|| reason.to_string());
                continue;
            }
            if contains_glob(&pattern) {
                match GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .backslash_escape(false)
                    .build()
                {
                    Ok(pattern) => {
                        builder.add(pattern);
                    }
                    Err(reason) => {
                        error.get_or_insert_with(|| reason.to_string());
                    }
                }
            } else {
                exact.insert(pattern);
            }
        }
        let globs = builder.build().unwrap_or_else(|reason| {
            error.get_or_insert_with(|| reason.to_string());
            GlobSetBuilder::new().build().expect("empty glob set")
        });
        Self { exact, globs, error }
    }

    /// Returns whether the selector matches a path.
    pub fn matches(&self, path: &str) -> anyhow::Result<bool> {
        if let Some(error) = &self.error {
            return Err(anyhow!("invalid minify asset selector: {error}"));
        }
        let path = normalize(path);
        Ok(self.exact.contains(&path) || self.globs.is_match(path))
    }

    /// Returns configured exact paths.
    pub fn exact(&self) -> impl Iterator<Item = &String> {
        self.exact.iter()
    }

    /// Returns a compilation error, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Returns whether no valid selector was configured.
    pub fn is_empty(&self) -> bool {
        self.error.is_none() && self.exact.is_empty() && self.globs.is_empty()
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Normalizes one MkDocs-compatible configured asset path.
pub fn normalize(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_owned()
}

/// Returns whether a selector contains glob syntax.
fn contains_glob(pattern: &str) -> bool {
    pattern
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::Selector;

    #[test]
    fn supports_exact_paths_and_recursive_globs() {
        let selector = Selector::new(&[
            "scripts/app.js".into(),
            "vendor/**/*.js".into(),
            "scripts/café.js".into(),
        ]);
        assert!(selector.matches("./scripts/app.js").unwrap());
        assert!(selector.matches("/scripts/app.js").unwrap());
        assert!(selector.matches("scripts\\app.js").unwrap());
        assert!(selector.matches("vendor/lib/tool.js").unwrap());
        assert!(selector.matches("scripts/café.js").unwrap());
        assert!(!selector.matches("scripts/other.js").unwrap());
    }

    #[test]
    fn rejects_parent_traversal_and_empty_paths() {
        for path in ["", "../scripts/app.js", "scripts/../app.js"] {
            let selector = Selector::new(&[path.into()]);
            assert!(selector.matches("scripts/app.js").is_err(), "{path}");
        }
    }
}
