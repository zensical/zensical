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

//! Markdown rendering.

use pyo3::types::PyAnyMethods;
use pyo3::{FromPyObject, PyErr, Python};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zrx::id::Id;
use zrx::scheduler::action::report::IntoReport;
use zrx::scheduler::action::Error;
use zrx::scheduler::Value;

use crate::structure::dynamic::Dynamic;
use crate::structure::nav::to_title;
use crate::structure::search::SearchItem;
use crate::structure::toc::Section;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

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
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Markdown {
    /// Renders Markdown using Python Markdown.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
    pub fn new(id: &Id, content: String) -> impl IntoReport<Markdown> {
        let id = id.clone();
        Python::attach(|py| {
            let module = py.import("zensical.markdown")?;
            module
                .call_method1("render", (content, id.location()))?
                .extract::<Markdown>()
        })
        .map_err(|err: PyErr| Error::from(Box::new(err) as Box<_>))
        .map(|markdown| Markdown {
            title: extract_title(&id, &markdown),
            meta: markdown.meta,
            content: markdown.content,
            search: markdown.search,
            toc: markdown.toc,
        })
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
