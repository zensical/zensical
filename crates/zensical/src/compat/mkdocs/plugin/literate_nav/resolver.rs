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

//! Folder navigation resolution and wildcard expansion.

use anyhow::{Context, Result};
use globset::Glob;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::path::SourcePath;
use crate::structure::markdown::render_literate_nav;
use crate::structure::nav::{
    source_sort_key, to_title, Navigation, NavigationItem, Plan, PlanItem,
};
use crate::structure::page::Page;

use super::parser::{self, Item};
use super::Settings;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Navigation entry before wildcard expansion.
#[derive(Clone, Debug)]
enum Entry {
    /// Explicit page or external link.
    Reference {
        /// Optional display title supplied by configuration or Markdown.
        title: Option<String>,
        /// Normalized source path or unchanged external target.
        target: String,
    },
    /// Named group of child entries.
    Section {
        /// Section display title.
        title: String,
        /// Entries nested below the section.
        children: Vec<Entry>,
    },
    /// File or directory pattern expanded against the page catalog.
    Wildcard {
        /// Optional title wrapping all expanded matches.
        title: Option<String>,
        /// Root-relative normalized component pattern.
        pattern: String,
        /// Original unresolved target retained when no match is usable.
        fallback: Option<String>,
    },
    /// Folder link delegated to that folder's navigation document.
    Directory {
        /// Optional title wrapping the resolved folder contents.
        title: Option<String>,
        /// Normalized folder root used during recursive resolution.
        root: String,
        /// Original link target retained when recursion is rejected.
        fallback: String,
    },
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Ordered documentation file and directory index.
struct Catalog {
    /// Page source paths in MkDocs navigation order.
    files: Vec<String>,
    /// Ancestor directories in first-page occurrence order.
    directories: Vec<String>,
    /// Directory membership index used for folder-link recognition.
    directory_set: HashSet<String>,
    /// Preferred index source for each directory.
    indexes: HashMap<String, String>,
}

/// One complete navigation resolution.
struct Resolver<'a> {
    /// Immutable plugin and configured-navigation settings.
    settings: &'a Settings,
    /// Navigation Markdown keyed by canonical source-relative path.
    documents: &'a BTreeMap<String, String>,
    /// Ordered pages and their derived directory facts.
    files: Catalog,
    /// Explicit or expanded paths already consumed by navigation.
    seen: HashSet<String>,
    /// Recursive paths already reported during this resolution.
    warned: HashSet<String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Catalog {
    /// Builds MkDocs-compatible ordered file and directory collections.
    fn new(pages: &[Page]) -> Self {
        let paths = pages
            .iter()
            .map(|page| page.source().clone())
            .collect::<Vec<_>>();
        Self::from_paths(paths)
    }

    /// Builds the catalog from validated source paths.
    fn from_paths(mut paths: Vec<SourcePath>) -> Self {
        paths.sort_by_key(source_sort_key);

        let files = paths.iter().map(ToString::to_string).collect::<Vec<_>>();
        let mut directories = Vec::new();
        let mut directory_set = HashSet::new();
        let mut indexes = HashMap::new();
        for path in &paths {
            let mut parent = path
                .parent()
                .map_or_else(|| ".".into(), |path| path.to_string());
            if matches!(path.file_name(), "index.md" | "README.md") {
                indexes.insert(parent.clone(), path.to_string());
            }
            loop {
                if directory_set.insert(parent.clone()) {
                    directories.push(parent.clone());
                }
                if parent == "." {
                    break;
                }
                parent = parent_path(&parent);
            }
        }
        Self {
            files,
            directories,
            directory_set,
            indexes,
        }
    }

    /// Returns whether a normalized path identifies a page ancestor.
    fn is_dir(&self, path: &str) -> bool {
        self.directory_set.contains(&normalize_directory(path))
    }

    /// Returns the preferred index source for one directory.
    fn find_index(&self, root: &str) -> Option<&str> {
        self.indexes
            .get(&normalize_directory(root))
            .map(String::as_str)
    }

    /// Returns file matches before directory matches, preserving source order.
    fn matches(&self, pattern: &str) -> Result<Vec<String>> {
        let pattern = pattern.trim_end_matches('/');
        let expected = components(pattern);
        let patterns = expected
            .iter()
            .map(|part| Glob::new(part).map(|glob| glob.compile_matcher()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let candidates = self
            .files
            .iter()
            .chain(&self.directories)
            .filter(|candidate| {
                let actual = components(candidate);
                actual.len() == patterns.len()
                    && actual
                        .iter()
                        .zip(&patterns)
                        .all(|(part, matcher)| matcher.is_match(part))
            })
            .cloned()
            .collect();
        Ok(candidates)
    }
}

impl<'a> Resolver<'a> {
    /// Creates one isolated resolution over settled documents and pages.
    fn new(
        settings: &'a Settings, documents: &'a BTreeMap<String, String>,
        pages: &[Page],
    ) -> Self {
        Self {
            settings,
            documents,
            files: Catalog::new(pages),
            seen: HashSet::new(),
            warned: HashSet::new(),
        }
    }

    /// Resolves root literate navigation or falls back to configured nav.
    fn resolve(&mut self) -> Result<Plan> {
        let root_document = join(".", &self.settings.nav_file);
        if self.settings.configured.is_empty()
            || self.documents.contains_key(&root_document)
        {
            let items = self.markdown_to_nav(".", &[String::from(".")])?;
            if !items.is_empty() {
                return Ok(Plan::new(items));
            }
        }

        let configured = self
            .settings
            .configured
            .iter()
            .map(|item| self.configured(item))
            .collect::<Vec<_>>();
        Ok(Plan::new(self.expand(configured, &[String::from(".")])?))
    }

    /// Resolves one folder's Markdown list or inferred contents.
    fn markdown_to_nav(
        &mut self, root: &str, roots: &[String],
    ) -> Result<Vec<PlanItem>> {
        let document_path = join(root, &self.settings.nav_file);
        if let Some(content) = self.documents.get(&document_path) {
            let html = render_literate_nav(content)
                .with_context(|| format!("failed to render {document_path}"))?;
            if let Some(items) = parser::parse(&html)
                .with_context(|| format!("failed to parse {document_path}"))?
            {
                if !(self.settings.implicit_index
                    && self.files.find_index(root) == Some(&document_path))
                {
                    self.seen.insert(document_path);
                }
                let mut entries = Vec::new();
                if self.settings.implicit_index
                    && let Some(index) = self.files.find_index(root)
                {
                    entries.push(Entry::Wildcard {
                        title: None,
                        pattern: index.to_owned(),
                        fallback: None,
                    });
                }
                entries.extend(
                    items
                        .into_iter()
                        .map(|item| self.parsed(root, item))
                        .collect::<Vec<_>>(),
                );
                return self.expand(entries, roots);
            }
        }

        let entries = vec![Entry::Wildcard {
            title: None,
            pattern: wildcard(root, "*", false),
            fallback: None,
        }];
        self.expand(entries, roots)
    }

    /// Converts a parsed Markdown item and records explicit references.
    fn parsed(&mut self, root: &str, item: Item) -> Entry {
        match item {
            Item::Reference { title, target } => {
                self.reference(root, title, target)
            }
            Item::Section { title, children } => Entry::Section {
                title,
                children: children
                    .into_iter()
                    .map(|item| self.parsed(root, item))
                    .collect(),
            },
            Item::Wildcard(pattern) => Entry::Wildcard {
                title: None,
                pattern: wildcard(root, &pattern, true),
                fallback: Some(pattern),
            },
        }
    }

    /// Converts configured MkDocs navigation and records explicit references.
    fn configured(&mut self, item: &NavigationItem) -> Entry {
        if !item.children.is_empty() {
            return Entry::Section {
                title: item.title.clone().unwrap_or_default(),
                children: item
                    .children
                    .iter()
                    .map(|item| self.configured(item))
                    .collect(),
            };
        }
        let target = item.url.clone().unwrap_or_default();
        if target.contains('*') {
            Entry::Wildcard {
                title: item.title.clone(),
                pattern: wildcard("", &target, true),
                fallback: Some(target),
            }
        } else if item.title.is_some() {
            self.reference("", item.title.clone(), target)
        } else {
            self.seen.insert(target.clone());
            Entry::Reference { title: None, target }
        }
    }

    /// Converts a link into an external/page reference or folder insertion.
    fn reference(
        &mut self, root: &str, title: Option<String>, target: String,
    ) -> Entry {
        if is_external(&target) {
            return Entry::Reference { title, target };
        }
        let absolute = join(root, &target);
        self.seen.insert(absolute.clone());
        if target.ends_with('/') && self.files.is_dir(&absolute) {
            Entry::Directory {
                title,
                root: normalize_directory(&absolute),
                fallback: target,
            }
        } else {
            Entry::Reference { title, target: absolute }
        }
    }

    /// Expands all wildcard and folder entries depth first.
    fn expand(
        &mut self, entries: Vec<Entry>, roots: &[String],
    ) -> Result<Vec<PlanItem>> {
        let mut resolved = Vec::new();
        for entry in entries {
            match entry {
                Entry::Reference { title, target } => {
                    resolved.push(PlanItem::reference(title, target));
                }
                Entry::Section { title, children } => {
                    let children = self.expand(children, roots)?;
                    if !children.is_empty() {
                        resolved.push(PlanItem::section(title, children));
                    }
                }
                Entry::Directory { title, root, fallback } => {
                    if roots.iter().any(|value| value == &root) {
                        self.warn_recursion(&root, roots);
                        resolved.push(PlanItem::reference(title, fallback));
                        continue;
                    }
                    let mut next = Vec::with_capacity(roots.len() + 1);
                    next.push(root.clone());
                    next.extend_from_slice(roots);
                    let children = self.markdown_to_nav(&root, &next)?;
                    if let Some(title) = title
                        && !children.is_empty()
                    {
                        resolved.push(PlanItem::section(title, children));
                    } else if !children.is_empty() {
                        resolved.extend(children);
                    }
                }
                Entry::Wildcard { title, pattern, fallback } => {
                    let (mut expanded, matched) =
                        self.expand_wildcard(&pattern, roots)?;
                    let mut used_fallback = false;
                    if expanded.is_empty()
                        && (title.is_some() || !matched)
                        && let Some(fallback) = fallback
                    {
                        expanded.push(PlanItem::reference(None, fallback));
                        used_fallback = true;
                    }
                    if let Some(title) = title {
                        if used_fallback
                            && expanded.len() == 1
                            && let PlanItem::Reference {
                                title: item_title, ..
                            } = &mut expanded[0]
                        {
                            *item_title = Some(title);
                            resolved.extend(expanded);
                            continue;
                        }
                        if !expanded.is_empty() {
                            resolved.push(PlanItem::section(title, expanded));
                        }
                    } else {
                        resolved.extend(expanded);
                    }
                }
            }
        }
        Ok(resolved)
    }

    /// Reports each rejected recursive path once per navigation resolution.
    fn warn_recursion(&mut self, root: &str, roots: &[String]) {
        let mut path = Vec::with_capacity(roots.len() + 1);
        path.push(root);
        path.extend(roots.iter().map(String::as_str));
        path.reverse();
        let path = path
            .into_iter()
            .map(|item| format!("{item:?}"))
            .collect::<Vec<_>>()
            .join(" -> ");
        if self.warned.insert(path.clone()) {
            eprintln!("WARNING -  Disallowing recursion {path}");
        }
    }

    /// Expands one pattern while excluding paths consumed by earlier entries.
    fn expand_wildcard(
        &mut self, pattern: &str, roots: &[String],
    ) -> Result<(Vec<PlanItem>, bool)> {
        let mut resolved = Vec::new();
        let candidates = self.files.matches(pattern)?;
        let any_match = !candidates.is_empty();
        for item in candidates {
            if self.seen.contains(&item) {
                continue;
            }
            if self.files.is_dir(&item) {
                let mut next = Vec::with_capacity(roots.len() + 1);
                next.push(item.clone());
                next.extend_from_slice(roots);
                let children = self.markdown_to_nav(&item, &next)?;
                if !children.is_empty() {
                    let title = to_title(item.rsplit('/').next().unwrap_or(""));
                    resolved.push(PlanItem::section(title, children));
                }
            } else if pattern.ends_with('/') {
                continue;
            } else {
                resolved.push(PlanItem::reference(None, item.clone()));
            }
            self.seen.insert(item);
        }
        Ok((resolved, any_match))
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Resolves native literate navigation and attaches rendered page facts.
pub fn resolve(
    settings: &Settings, documents: &BTreeMap<String, String>, pages: &[Page],
) -> Result<Navigation> {
    let plan = Resolver::new(settings, documents, pages).resolve()?;
    Ok(plan.compile(pages.to_vec()))
}

/// Joins and normalizes two POSIX paths while preserving a leading slash.
fn join(root: &str, target: &str) -> String {
    let absolute = target.starts_with('/');
    let source = if absolute || root.is_empty() || root == "." {
        target.to_owned()
    } else if target.is_empty() {
        root.to_owned()
    } else {
        format!("{root}/{target}")
    };
    normalize(&source, absolute)
}

/// Resolves dot segments without allowing absolute paths above their root.
fn normalize(source: &str, absolute: bool) -> String {
    let mut output = Vec::new();
    for component in source.split('/') {
        match component {
            ".." if output.last().is_some_and(|item| *item != "..") => {
                output.pop();
            }
            ".." if !absolute => output.push(component),
            "" | "." | ".." => {}
            _ => output.push(component),
        }
    }
    let output = output.join("/");
    if absolute {
        format!("/{output}")
    } else if output.is_empty() {
        ".".into()
    } else {
        output
    }
}

/// Normalizes a path for comparison with source-relative directories.
fn normalize_directory(path: &str) -> String {
    normalize(path.trim_start_matches('/'), false)
}

/// Joins a wildcard to its folder and optionally retains a trailing slash.
fn wildcard(root: &str, pattern: &str, preserve_slash: bool) -> String {
    let trailing = preserve_slash && pattern.ends_with('/');
    let value = normalize_directory(&join(root, pattern));
    if trailing {
        format!("{value}/")
    } else {
        value
    }
}

/// Returns the normalized parent path, using `.` for the source root.
fn parent_path(path: &str) -> String {
    path.rsplit_once('/').map_or(".".into(), |(parent, _)| {
        if parent.is_empty() {
            ".".into()
        } else {
            parent.into()
        }
    })
}

/// Splits a normalized path into meaningful source components.
fn components(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

/// Returns whether a link target has a URL scheme or network-path prefix.
fn is_external(target: &str) -> bool {
    if target.starts_with("//") {
        return true;
    }
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme.chars().enumerate().all(|(index, value)| {
            value.is_ascii_alphabetic()
                || (index > 0
                    && (value.is_ascii_digit()
                        || matches!(value, '+' | '-' | '.')))
        })
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{is_external, join, wildcard, Catalog};

    #[test]
    fn catalogs_pages_in_navigation_order_and_prefers_index() {
        let catalog = Catalog::from_paths(
            [
                "guide/z.md",
                "root.md",
                "guide/README.md",
                "guide/index.md",
                "guide/a.md",
                "index.md",
            ]
            .into_iter()
            .map(|path| path.parse().unwrap())
            .collect(),
        );

        assert_eq!(
            catalog.matches("guide/*.md").unwrap(),
            [
                "guide/README.md",
                "guide/index.md",
                "guide/a.md",
                "guide/z.md",
            ]
        );
        assert!(catalog.is_dir("guide/"));
        assert_eq!(catalog.find_index("guide"), Some("guide/index.md"));
    }

    #[test]
    fn normalizes_links_and_wildcards() {
        assert_eq!(join("guide", "../index.md"), "index.md");
        assert_eq!(join("guide", "/outside.md"), "/outside.md");
        assert_eq!(join(".", "../../page.md"), "../../page.md");
        assert_eq!(join("guide", "../../page.md"), "../page.md");
        assert_eq!(wildcard("guide", "../*.md", true), "*.md");
        assert_eq!(wildcard("guide", "sub/", true), "guide/sub/");
    }

    #[test]
    fn detects_url_schemes_and_network_paths() {
        assert!(is_external("https://example.com"));
        assert!(is_external("mailto:test@example.com"));
        assert!(is_external("//example.com/path"));
        assert!(!is_external("guide/page.md"));
    }
}
