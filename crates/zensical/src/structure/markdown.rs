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

//! Markdown rendering.

use pyo3::types::PyAnyMethods;
use pyo3::{FromPyObject, PyErr, Python};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zrx::id::Id;
use zrx::scheduler::action::{Error, Report};
use zrx::scheduler::Value;
use zrx_diagnostic::{Diagnostic, Severity};

use crate::structure::dynamic::Dynamic;
use crate::structure::nav::to_title;
use crate::structure::search::SearchItem;
use crate::structure::toc::Section;

mod autorefs;

pub use autorefs::Autorefs;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// A logging-level diagnostic from the Python Markdown render (see Python
/// `logging` level constants, e.g. WARNING = 30, ERROR = 40).
#[derive(Clone, Debug, FromPyObject, Serialize, Deserialize, PartialEq, Eq)]
#[pyo3(from_item_all)]
pub struct RenderDiagnostic {
    /// Python `logging` level (`LogRecord.levelno`), e.g. 30 = WARNING, 40 = ERROR.
    pub level: u8,
    /// Formatted message (e.g. `logger: text`).
    pub message: String,
}

/// Markdown.
#[derive(Clone, Debug, FromPyObject, Serialize, Deserialize)]
#[pyo3(from_item_all)]
pub struct Markdown {
    /// Markdown metadata.
    pub meta: BTreeMap<String, Dynamic>,
    /// Markdown content.
    pub content: String,
    /// Search index.
    pub search: Vec<SearchItem>,
    /// Page title extracted from Markdown.
    pub title: String,
    /// Table of contents.
    pub toc: Vec<Section>,
    /// Messages collected during Python render
    #[serde(default)]
    pub render_diagnostics: Vec<RenderDiagnostic>,
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Map Python level numbers to [`Severity`].
fn logging_level_to_severity(level: u8) -> Severity {
    match level {
        40 | 50 => Severity::Error, // ERROR, CRITICAL
        30 => Severity::Warning,
        20 => Severity::Info,
        10 => Severity::Debug,
        _ if level > 50 => Severity::Error,
        _ => Severity::Debug,
    }
}

/// Wrap [`Markdown`] in a [`Report`] with diagnostics from [`Markdown::render_diagnostics`]
/// (severity follows Python log level). Used after cache hits (see [`crate::workflow::cached`]).
pub(crate) fn markdown_into_report(markdown: Markdown) -> Report<Markdown> {
    let diagnostics: Vec<Diagnostic> = markdown
        .render_diagnostics
        .iter()
        .map(|d| {
            Diagnostic::new(
                logging_level_to_severity(d.level),
                d.message.clone(),
            )
        })
        .collect();
    Report::new(markdown).with(diagnostics)
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Markdown {
    /// Renders Markdown using Python Markdown.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
    pub fn new(
        id: &Id, url: String, content: String,
    ) -> Result<Report<Markdown>, Error> {
        let id = id.clone();
        Python::attach(|py| -> Result<Report<Markdown>, PyErr> {
            let module = py.import("zensical.markdown")?;
            let m: Markdown = module
                .call_method1("render", (content, id.location(), url))?
                .extract()?;
            let markdown = Markdown {
                title: extract_title(&id, &m),
                meta: m.meta,
                content: m.content,
                search: m.search,
                toc: m.toc,
                render_diagnostics: m.render_diagnostics,
            };
            Ok(markdown_into_report(markdown))
        })
        .map_err(|err: PyErr| Error::from(Box::new(err) as Box<_>))
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Markdown {}

// ----------------------------------------------------------------------------

impl PartialEq for Markdown {
    fn eq(&self, other: &Self) -> bool {
        self.content == other.content
    }
}

impl Eq for Markdown {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Extract the title from the metadata or table of contents.
///
/// MkDocs prioritizes the "title" metadata field over the actual title in the
/// page. This has been a huge source of confusion, as can be read here:
/// https://github.com/mkdocs/mkdocs/issues/3532
///
/// We'll fix this in our modular navigation proposal that will make title
/// handling much more flexible in the near future.
fn extract_title(id: &Id, markdown: &Markdown) -> String {
    if let Some(value) = markdown.meta.get("title") {
        return value.to_string();
    }

    // Otherwise, fall back to the first top-level heading, if existent
    let mut iter = markdown.toc.iter();
    if let Some(item) = iter.find(|item| item.level == 1) {
        return item.title.clone();
    }

    // As a last resort, use the file name
    let location = id.location();

    // Split location into components at slashes
    let mut components = location
        .split('/')
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    // Extract file, and return title
    let file = components.pop().expect("invariant");
    to_title(&file)
}

#[cfg(test)]
mod tests {
    use super::{markdown_into_report, Markdown, RenderDiagnostic};
    use std::collections::BTreeMap;
    use zrx::scheduler::action::Report;
    use zrx_diagnostic::Severity;

    #[test]
    fn markdown_into_report_maps_warning_level() {
        let md = Markdown {
            meta: BTreeMap::new(),
            content: String::new(),
            search: vec![],
            title: String::new(),
            toc: vec![],
            render_diagnostics: vec![RenderDiagnostic {
                level: 30,
                message: "griffe: example".to_string(),
            }],
        };
        let report: Report<Markdown> = markdown_into_report(md);
        let warnings: Vec<_> = report
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].message, "griffe: example");
    }

    #[test]
    fn markdown_into_report_maps_error_level() {
        let md = Markdown {
            meta: BTreeMap::new(),
            content: String::new(),
            search: vec![],
            title: String::new(),
            toc: vec![],
            render_diagnostics: vec![RenderDiagnostic {
                level: 40,
                message: "mkdocstrings: failed".to_string(),
            }],
        };
        let report: Report<Markdown> = markdown_into_report(md);
        let errors: Vec<_> = report
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "mkdocstrings: failed");
    }
}
