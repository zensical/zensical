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

//! Issues.

use ahash::{HashMap, HashSet};
use ariadne::{Color, Config, IndexType, Label, Report, ReportKind, Source};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::slice::Iter;
use zrx::id::Id;
use zrx::scheduler::{Key, Value};

use super::collector::reference::Reference;
use super::collector::{Anchors, References};
use super::span::Span;

mod error;

pub use error::{Error, Result};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    /// Link or image reference with no matching definition.
    UnresolvedReference {
        path: PathBuf,
        span: Span,
        id: String,
    },
    /// Footnote reference with no matching definition.
    UnresolvedFootnote {
        path: PathBuf,
        span: Span,
        id: String,
    },
    /// Link definition that is never referenced.
    UnusedDefinition {
        path: PathBuf,
        span: Span,
        id: String,
    },
    /// Footnote definition that is never referenced.
    UnusedFootnote {
        path: PathBuf,
        span: Span,
        id: String,
    },
    /// Invalid link.
    InvalidLink {
        path: PathBuf,
        span: Span,
        href: String,
    },
    /// Invalid link anchor
    InvalidLinkAnchor {
        path: PathBuf,
        span: Span,
        href: String,
        anchor: String,
    },
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Issues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issues {
    /// Markdown contents for printing errors.
    contents: HashMap<String, String>,
    /// Inner set of issues.
    inner: Vec<Issue>,
}

// ----------------------------------------------------------------------------

impl Issue {
    /// Returns the path of the issue.
    pub fn path(&self) -> &Path {
        match self {
            Issue::UnresolvedReference { path, .. }
            | Issue::UnresolvedFootnote { path, .. }
            | Issue::UnusedDefinition { path, .. }
            | Issue::UnusedFootnote { path, .. }
            | Issue::InvalidLink { path, .. }
            | Issue::InvalidLinkAnchor { path, .. } => path,
        }
    }

    /// Returns the span of the issue.
    pub fn span(&self) -> &Span {
        match self {
            Issue::UnresolvedReference { span, .. }
            | Issue::UnresolvedFootnote { span, .. }
            | Issue::UnusedDefinition { span, .. }
            | Issue::UnusedFootnote { span, .. }
            | Issue::InvalidLink { span, .. }
            | Issue::InvalidLinkAnchor { span, .. } => span,
        }
    }
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Issues {
    /// Create a new set of issues.
    #[allow(clippy::too_many_lines)]
    pub fn new<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (Key<Id>, (References, Anchors))>,
    {
        let mut issues = Vec::new();
        let mut contents = HashMap::default();

        // Create link map and anchor map and find inner-page issues
        let mut link_map = HashMap::default();
        let mut anchor_map = HashMap::default();
        for (key, (references, anchors)) in iter {
            let id = key.try_as_id().expect("invariant");
            let path = id.location().into_owned();

            // Associate anchors with their location for lookup
            contents.insert(path.clone(), references.markdown().to_string());
            anchor_map.insert(
                path.clone(),
                anchors.into_iter().cloned().collect::<HashSet<_>>(),
            );

            // Collect all links for each page for cross-page checking later
            let mut mappings = Vec::new();
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            for reference in &references {
                if let Reference::Link(link) = reference {
                    let href =
                        &references.markdown()[link.href.start..link.href.end];
                    if !href.starts_with("http://")
                        && !href.starts_with("https://")
                    {
                        mappings.push((link.href, href.to_string()));
                    }
                }
                if let Reference::LinkDefinition(link) = reference {
                    let href =
                        &references.markdown()[link.href.start..link.href.end];
                    if !href.starts_with("http://")
                        && !href.starts_with("https://")
                    {
                        mappings.push((link.href, href.to_string()));
                    }
                }
            }
            link_map.insert(id.location().into_owned(), mappings);

            // Initialize link and footnote definitions
            let mut link_defs = HashMap::default();
            let mut note_defs = HashMap::default();

            // 1st pass - collect link and footnote definitions
            let markdown = references.markdown();
            for reference in &references {
                match reference {
                    Reference::LinkDefinition(link) => {
                        let id = &markdown[link.id.start..link.id.end];
                        link_defs.insert(to_id(id), link);
                    }
                    Reference::FootnoteDefinition(footnote) => {
                        let id = &markdown[footnote.id.start..footnote.id.end];
                        note_defs.insert(to_id(id), footnote);
                    }
                    _ => {}
                }
            }

            // Initialize used link and footnote definitions
            let mut used_link_defs = HashSet::default();
            let mut used_note_defs = HashSet::default();

            // 2nd pass - check link and footnote references
            for reference in &references {
                match reference {
                    Reference::LinkReference(link) => {
                        let id = &markdown[link.id.start..link.id.end];
                        if link_defs.contains_key(&to_id(id)) {
                            used_link_defs.insert(to_id(id));
                        } else {
                            issues.push(Issue::UnresolvedReference {
                                path: path.clone().into(),
                                span: (link.id.start..link.id.end).into(),
                                id: id.to_string(),
                            });
                        }
                    }
                    Reference::FootnoteReference(note) => {
                        let id = &markdown[note.id.start..note.id.end];
                        if note_defs.contains_key(&to_id(id)) {
                            used_note_defs.insert(to_id(id));
                        } else {
                            issues.push(Issue::UnresolvedFootnote {
                                path: path.clone().into(),
                                span: (note.id.start..note.id.end).into(),
                                id: id.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }

            // Collect all remaining link definitions as unused
            for link in link_defs.into_values() {
                let id = &markdown[link.id.start..link.id.end];
                if !used_link_defs.contains(&to_id(id)) {
                    issues.push(Issue::UnusedDefinition {
                        path: path.clone().into(),
                        span: (link.id.start..link.id.end).into(),
                        id: id.to_string(),
                    });
                }
            }

            // Collect all remaining footnote definitions as unused
            for note in note_defs.into_values() {
                let id = &markdown[note.id.start..note.id.end];
                if !used_note_defs.contains(&to_id(id)) {
                    issues.push(Issue::UnusedFootnote {
                        path: path.clone().into(),
                        span: (note.id.start..note.id.end).into(),
                        id: id.to_string(),
                    });
                }
            }
        }

        // Check links across pages for issues
        for (base, mappings) in link_map {
            let base = Path::new(&base);
            for (span, href) in mappings {
                if let Some((path, anchor)) = href.split_once('#') {
                    if !Path::new(path)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                    {
                        continue;
                    }
                    let link = resolve_relative(base, path)
                        .to_string_lossy()
                        .into_owned();

                    // Check if the link exists, and if it does, whether the
                    // anchor exists on the target page
                    if let Some(anchors) = anchor_map.get(&link) {
                        if !anchors.contains(anchor) {
                            issues.push(Issue::InvalidLinkAnchor {
                                path: base.into(),
                                span: Span::from(
                                    (span.start + path.len() + 1)..span.end,
                                ),
                                href: href.clone(),
                                anchor: anchor.to_string(),
                            });
                        }
                    } else {
                        issues.push(Issue::InvalidLink {
                            path: base.into(),
                            span,
                            href: link,
                        });
                    }
                } else {
                    if !Path::new(&href)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                    {
                        continue;
                    }
                    let link = resolve_relative(base, &href)
                        .to_string_lossy()
                        .into_owned();

                    if !anchor_map.contains_key(&link) {
                        issues.push(Issue::InvalidLink {
                            path: base.into(),
                            span,
                            href: link,
                        });
                    }
                }
            }
        }

        // Sort issues by path, then by span
        issues.sort_by(|a, b| {
            a.path()
                .cmp(b.path())
                .then_with(|| a.span().start.cmp(&b.span().start))
        });

        // Return issues
        Self { contents, inner: issues }
    }

    /// Prints the issue to stderr.
    #[allow(clippy::match_same_arms)]
    pub fn print(&self, strict: bool) -> Result {
        for issue in &self.inner {
            // Determine the path and kind of report
            let path = issue.path().to_string_lossy();
            let kind = match issue {
                Issue::UnresolvedReference { .. } => ReportKind::Warning,
                Issue::UnresolvedFootnote { .. } => ReportKind::Warning,
                Issue::UnusedDefinition { .. } => ReportKind::Warning,
                Issue::UnusedFootnote { .. } => ReportKind::Warning,
                Issue::InvalidLink { .. } => ReportKind::Warning,
                Issue::InvalidLinkAnchor { .. } => ReportKind::Warning,
            };

            // Determine the label message and color
            let (message, color) = match issue {
                Issue::UnresolvedReference { .. } => {
                    ("unresolved link reference", Color::Yellow)
                }
                Issue::UnresolvedFootnote { .. } => {
                    ("unresolved footnote reference", Color::Yellow)
                }
                Issue::UnusedDefinition { .. } => {
                    ("unused link definition", Color::Yellow)
                }
                Issue::UnusedFootnote { .. } => {
                    ("unused footnote definition", Color::Yellow)
                }
                Issue::InvalidLink { .. } => {
                    ("page does not exist", Color::Yellow)
                }
                Issue::InvalidLinkAnchor { .. } => {
                    ("anchor does not exist", Color::Yellow)
                }
            };

            // Create report
            let builder = Report::build(
                kind,
                (path.as_ref(), Range::from(*issue.span())),
            )
            .with_message(message)
            .with_label(
                Label::new((path.as_ref(), Range::from(*issue.span())))
                    .with_message(message)
                    .with_color(color),
            );

            // Obtain Markdown source
            let source = self
                .contents()
                .get(&issue.path().to_string_lossy().to_string())
                .cloned()
                .unwrap_or_default();

            // Create and print report
            builder
                .with_config(Config::default().with_index_type(IndexType::Byte))
                .finish()
                .eprint((path.as_ref(), Source::from(source)))?;
        }

        // Print summary, if any issues were found
        if !self.is_empty() {
            let s = if self.len() == 1 { "" } else { "s" };
            eprintln!("{} issue{s} found", self.len());
            if strict {
                return Err(Error::Strict);
            }
        }
        Ok(())
    }

    /// Returns the Markdown contents.
    pub fn contents(&self) -> &HashMap<String, String> {
        &self.contents
    }

    /// Returns the number of issues.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether there are no issues.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Issues {}

// ----------------------------------------------------------------------------

impl<'a> IntoIterator for &'a Issues {
    type Item = &'a Issue;
    type IntoIter = Iter<'a, Issue>;

    /// Creates an iterator over the issues.
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Converts an id to a normalized form for comparison.
fn to_id(id: &str) -> String {
    let iter = id.split_whitespace();
    iter.collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Normalizes a path by removing `.` and resolving `..`.
fn normalize(path: PathBuf) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                components.pop();
            }
            Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}

/// Resolves a relative URL against a base path.
fn resolve_relative<P>(base: P, href: &str) -> PathBuf
where
    P: AsRef<Path>,
{
    if href.is_empty() {
        return base.as_ref().to_path_buf();
    }
    let base_dir = base.as_ref().parent().unwrap_or(Path::new(""));
    normalize(base_dir.join(href))
}
