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

use anyhow::Result;
use pyo3::types::{PyAnyMethods, PyTracebackMethods};
use pyo3::{FromPyObject, Python};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::Value;

use crate::path::SourcePath;
use crate::structure::dynamic::Dynamic;
use crate::structure::nav::to_title;
use crate::structure::toc::Section;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Markdown.
///
/// The rendered payload is shared with the page derived from it. This keeps
/// the stream callback borrowed while making the Markdown-to-page handoff a
/// constant-sized clone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Markdown {
    /// Immutable rendered Markdown data.
    #[serde(flatten)]
    data: Arc<MarkdownData>,
}

/// Immutable rendered Markdown data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarkdownData {
    /// Markdown metadata.
    pub meta: BTreeMap<String, Dynamic>,
    /// Markdown content.
    pub content: String,
    /// Page title extracted from Markdown.
    pub title: String,
    /// Table of contents.
    pub toc: Vec<Section>,
}

/// Markdown data returned by the Python renderer.
#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct RenderedMarkdown {
    /// Markdown metadata.
    meta: BTreeMap<String, Dynamic>,
    /// Markdown content.
    content: String,
    /// Page title extracted from Markdown.
    title: String,
    /// Table of contents.
    toc: Vec<Section>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Markdown {
    /// Renders Markdown using Python Markdown.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
    pub fn new(
        id: &Id, url: String, content: String, meta: BTreeMap<String, Dynamic>,
    ) -> Result<Markdown> {
        let id = id.clone();
        let meta = serde_json::to_string(&meta)?;
        let res = Python::attach(|py| {
            let module = py.import("zensical.markdown.render")?;
            module
                .call_method1("render", (content, id.location(), url, meta))?
                .extract::<RenderedMarkdown>()
        })
        .map_err(|err| {
            Python::attach(|py| {
                let traceback = err
                    .traceback(py)
                    .and_then(|tb| tb.format().ok())
                    .unwrap_or_default();
                anyhow::anyhow!("Python error: {err}\n{traceback}")
            })
        });

        res.map(|data| {
            let mut data = MarkdownData {
                meta: data.meta,
                content: data.content,
                title: data.title,
                toc: data.toc,
            };
            data.title = extract_title(&id, &data);
            Markdown { data: Arc::new(data) }
        })
    }

    /// Replaces rendered HTML, cloning shared facts only when necessary.
    pub fn replace_content(&mut self, content: String) {
        Arc::make_mut(&mut self.data).content = content;
    }

    /// Applies optional derived HTML and TOC values with one copy-on-write.
    pub fn replace_derived(
        &mut self, content: Option<String>, toc: Option<Vec<Section>>,
    ) {
        if content.is_none() && toc.is_none() {
            return;
        }
        let data = Arc::make_mut(&mut self.data);
        if let Some(content) = content {
            data.content = content;
        }
        if let Some(toc) = toc {
            data.toc = toc;
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Markdown {}

// ----------------------------------------------------------------------------

impl Deref for Markdown {
    type Target = MarkdownData;

    /// Dereferences to immutable rendered Markdown data.
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

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
fn extract_title(id: &Id, markdown: &MarkdownData) -> String {
    if let Some(value) = markdown.meta.get("title")
        && !matches!(value, Dynamic::Null)
    {
        return value.to_string();
    }

    // Otherwise, fall back to the first top-level heading, if existent
    let mut iter = markdown.toc.iter();
    if let Some(item) = iter.find(|item| item.level == 1) {
        return item.title.clone();
    }

    // As a last resort, use the provider-relative file name.
    let source = id
        .location()
        .parse::<SourcePath>()
        .expect("Markdown source identity is canonical");
    to_title(source.file_name())
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::{Markdown, MarkdownData};

    fn markdown() -> Markdown {
        Markdown {
            data: Arc::new(MarkdownData {
                meta: BTreeMap::new(),
                content: String::from("<h1>Home</h1>"),
                title: String::from("Home"),
                toc: Vec::new(),
            }),
        }
    }

    #[test]
    fn clone_shares_immutable_data() {
        let markdown = markdown();
        let clone = markdown.clone();

        assert!(Arc::ptr_eq(&markdown.data, &clone.data));
    }

    #[test]
    fn serialization_keeps_flat_markdown_shape() {
        let value = serde_json::to_value(markdown()).unwrap();

        assert_eq!(value["content"], "<h1>Home</h1>");
        assert_eq!(value["title"], "Home");
        assert!(value.get("data").is_none());
        assert!(value.get("search").is_none());

        let markdown: Markdown = serde_json::from_value(value).unwrap();
        assert_eq!(markdown.content, "<h1>Home</h1>");
        assert_eq!(markdown.title, "Home");
    }
}
