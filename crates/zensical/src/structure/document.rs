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

//! Pre-render document facts.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zrx::stream::Value;

use crate::path::SourcePath;

use super::dynamic::Dynamic;
use super::nav::to_title;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Source document after metadata resolution and before Markdown rendering.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentHeader {
    /// Documentation-relative source identity.
    pub source: SourcePath,
    /// Markdown body with front matter removed.
    pub body: String,
    /// Resolved inherited and page-local metadata.
    pub meta: BTreeMap<String, Dynamic>,
    /// MkDocs-compatible title available before rendering.
    pub title: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl DocumentHeader {
    /// Creates pre-render facts for one Markdown source.
    pub fn new(
        source: SourcePath, body: String, meta: BTreeMap<String, Dynamic>,
    ) -> Self {
        let title = title(&source, &body, &meta);
        Self { source, body, meta, title }
    }
}

impl Value for DocumentHeader {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Resolves the title observable before MkDocs renders a page.
///
/// MkDocs' pre-render fallback deliberately recognizes only an H1 at the first
/// non-empty source line. Its full renderer can later refine ordinary page
/// titles, but Material's blog routes are computed from this earlier value.
fn title(
    source: &SourcePath, body: &str, meta: &BTreeMap<String, Dynamic>,
) -> String {
    if let Some(value) = meta.get("title")
        && !matches!(value, Dynamic::Null)
    {
        return value.to_string();
    }

    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    for line in normalized.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(title) = line.strip_prefix("# ") {
            return title.trim_start_matches(['#', ' ']).to_owned();
        }
        break;
    }

    if source.depth() == 1
        && matches!(source.file_name(), "index.md" | "README.md")
    {
        return "Home".into();
    }
    to_title(source.file_name())
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::DocumentHeader;
    use crate::structure::dynamic::Dynamic;

    fn document(path: &str, body: &str) -> DocumentHeader {
        DocumentHeader::new(path.parse().unwrap(), body.into(), BTreeMap::new())
    }

    #[test]
    fn metadata_title_has_precedence() {
        let mut meta = BTreeMap::new();
        meta.insert("title".into(), Dynamic::String("Metadata".into()));
        let document = DocumentHeader::new(
            "post.md".parse().unwrap(),
            "# Heading".into(),
            meta,
        );

        assert_eq!(document.title, "Metadata");
    }

    #[test]
    fn matches_mkdocs_pre_render_heading_rules() {
        assert_eq!(document("post.md", "\n# Heading\n").title, "Heading");
        assert_eq!(document("post.md", "# ## Heading").title, "Heading");
        assert_eq!(document("post.md", "## Heading").title, "Post");
        assert_eq!(document("post.md", "#Heading").title, "Post");
        assert_eq!(document("post.md", "Intro\n\n# Heading").title, "Post");
        assert_eq!(document("post.md", "Setext\n======").title, "Post");
    }

    #[test]
    fn falls_back_to_homepage_or_filename() {
        assert_eq!(document("index.md", "No heading").title, "Home");
        assert_eq!(document("README.md", "No heading").title, "Home");
        assert_eq!(
            document("guides/my-post.md", "No heading").title,
            "My post"
        );
    }
}
