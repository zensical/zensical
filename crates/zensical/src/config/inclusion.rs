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

//! File inclusion rules for MkDocs configuration.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::sync::Arc;

use super::Project;

/// Compiled gitignore rules shared by sources, assets and navigation.
#[derive(Clone, Debug)]
pub struct Inclusion {
    excluded: Patterns,
    drafts: Patterns,
    unlisted: Patterns,
}

/// An optional pattern set compiled by the existing pathspec dependency.
#[derive(Clone, Debug)]
struct Patterns(Option<Arc<Py<PyAny>>>);

impl Inclusion {
    /// The configured patterns are compiled once when configuration is loaded.
    pub fn new(project: &Project) -> PyResult<Self> {
        Ok(Self {
            excluded: Patterns::new(
                "exclude_docs",
                &format!(
                    ".*\n/templates/\n{}",
                    project.exclude_docs.as_deref().unwrap_or_default()
                ),
            )?,
            drafts: Patterns::new(
                "draft_docs",
                project.draft_docs.as_deref().unwrap_or_default(),
            )?,
            unlisted: Patterns::new(
                "not_in_nav",
                project.not_in_nav.as_deref().unwrap_or_default(),
            )?,
        })
    }

    /// Whether the file is included in the current build mode is returned.
    pub fn is_included(&self, path: &str, serve: bool) -> PyResult<bool> {
        Ok(!self.excluded.matches(path)? && (serve || !self.is_draft(path)?))
    }

    /// Whether the file is marked as a draft is returned.
    pub fn is_draft(&self, path: &str) -> PyResult<bool> {
        self.drafts.matches(path)
    }

    /// Whether the page is included in inferred navigation is returned.
    pub fn is_in_nav(&self, path: &str) -> PyResult<bool> {
        Ok(self.is_included(path, false)? && !self.unlisted.matches(path)?)
    }
}

impl Patterns {
    fn new(setting: &str, text: &str) -> PyResult<Self> {
        if text.is_empty() {
            return Ok(Self(None));
        }
        Python::attach(|py| {
            let patterns = py
                .import("pathspec")?
                .getattr("GitIgnoreSpec")?
                .call_method1("from_lines", (text.lines().collect::<Vec<_>>(),))
                .map_err(|error| {
                    PyValueError::new_err(format!(
                        "invalid {setting} patterns: {error}"
                    ))
                })?;
            Ok(Self(Some(Arc::new(patterns.unbind()))))
        })
    }

    fn matches(&self, path: &str) -> PyResult<bool> {
        let Some(patterns) = &self.0 else {
            return Ok(false);
        };
        Python::attach(|py| {
            patterns
                .bind(py)
                .call_method1("match_file", (path,))?
                .extract()
        })
    }
}
