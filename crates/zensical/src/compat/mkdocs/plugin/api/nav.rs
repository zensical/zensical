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

//! Navigation plans for generated API pages.
use super::{under, Section, Snapshot};
use crate::structure::nav::{Navigation, NavigationItem, NavigationResolution};
use crate::structure::page::Page;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Default)]
pub(super) struct Node {
    path: Option<String>,
    children: Vec<(String, Node)>,
}

impl Snapshot {
    /// Expands API sections before the normal navigation resolver runs.
    pub fn navigation(
        &self, configured: &[NavigationItem], pages: &[Page],
    ) -> Result<Vec<NavigationItem>> {
        if self.sections.is_empty() {
            return Ok(configured.to_vec());
        }
        let mut items = if configured.is_empty() {
            let ordinary: Vec<_> = pages
                .iter()
                .filter(|page| {
                    !self.sections.iter().any(|section| {
                        section.title.is_some()
                            && under(page.source().as_str(), &section.root)
                    })
                })
                .cloned()
                .collect();
            unresolved(&Navigation::from(ordinary), pages)
        } else {
            configured.to_vec()
        };
        for section in &self.sections {
            let mut children = section.children.clone();
            if !section.generated {
                children = unresolved(
                    &Navigation::from(
                        pages
                            .iter()
                            .filter(|page| {
                                under(page.source().as_str(), &section.root)
                            })
                            .cloned()
                            .collect::<Vec<_>>(),
                    ),
                    pages,
                );
                // Existing documentation is resolved through its summary by literate-nav.
            }
            let found = replace_section(&mut items, section, &children)?;
            if !found && let Some(title) = &section.title {
                let item = if section.generated {
                    item(Some(title.clone()), None, children)
                } else {
                    item(
                        Some(title.clone()),
                        Some(format!("{}/", section.root)),
                        Vec::new(),
                    )
                };
                items.push(item);
            }
        }
        Ok(items)
    }

    /// Appends API navigation after awesome-nav resolves ordinary pages.
    pub fn awesome(
        &self, resolution: &NavigationResolution, pages: &[Page],
    ) -> NavigationResolution {
        if self.sections.is_empty() {
            return resolution.clone();
        }
        let mut items = unresolved(&resolution.navigation, pages);
        for section in &self.sections {
            if let Some(title) = &section.title {
                items.push(item(
                    Some(title.clone()),
                    None,
                    section.children.clone(),
                ));
            }
        }
        Navigation::resolve(items, pages.to_vec())
    }

    /// Excludes automatically grouped API pages from awesome-nav's own scan.
    pub fn ordinary_pages(&self, pages: &[Page]) -> Vec<Page> {
        pages
            .iter()
            .filter(|page| {
                !self.sections.iter().any(|section| {
                    section.title.is_some()
                        && under(page.source().as_str(), &section.root)
                        && self.files.contains_key(page.source())
                })
            })
            .cloned()
            .collect()
    }
}

fn replace_section(
    items: &mut [NavigationItem], section: &Section,
    children: &[NavigationItem],
) -> Result<bool> {
    let mut found = false;
    for entry in items {
        let directory = entry
            .url
            .as_deref()
            .is_some_and(|url| url.trim_end_matches('/') == section.root);
        let title =
            section.autonav && entry.title.as_ref() == section.title.as_ref();
        let placeholder = section.autonav
            && entry.title.is_none()
            && entry.url.as_ref() == section.title.as_ref();
        if directory || title || placeholder {
            if title
                && entry.url.as_deref().is_some_and(|url| {
                    url.trim_end_matches('/') != section.root
                })
            {
                bail!(
                    "api-autonav navigation section {:?} must refer to {}",
                    section.title,
                    section.root
                );
            }
            if section.generated {
                entry.url = None;
                if placeholder {
                    entry.title.clone_from(&section.title);
                }
                entry.children.extend_from_slice(children);
            }
            found = true;
        } else {
            found |= replace_section(&mut entry.children, section, children)?;
        }
    }
    Ok(found)
}

fn unresolved(navigation: &Navigation, pages: &[Page]) -> Vec<NavigationItem> {
    fn restore(
        items: &mut [NavigationItem],
        sources: &BTreeMap<&str, &crate::path::SourcePath>,
    ) {
        for item in items {
            if let Some(source) =
                item.url.as_deref().and_then(|url| sources.get(url))
            {
                item.url = Some(source.to_string());
            }
            restore(&mut item.children, sources);
        }
    }
    let sources = pages
        .iter()
        .map(|page| (page.url.as_str(), page.source()))
        .collect();
    let mut items = navigation.items.as_ref().clone();
    restore(&mut items, &sources);
    items
}

fn item(
    title: Option<String>, url: Option<String>, children: Vec<NavigationItem>,
) -> NavigationItem {
    NavigationItem {
        title,
        url,
        children,
        canonical_url: None,
        meta: None,
        is_index: false,
        active: false,
    }
}

impl Node {
    pub(super) fn insert(&mut self, parts: &[String], path: String) {
        let mut node = self;
        for part in parts {
            let position = node
                .children
                .iter()
                .position(|(name, _)| name == part)
                .unwrap_or_else(|| {
                    node.children.push((part.clone(), Node::default()));
                    node.children.len() - 1
                });
            node = &mut node.children[position].1;
        }
        node.path = Some(path);
    }

    pub(super) fn items(
        &self, prefix: &str, full: bool, ancestors: &[String],
    ) -> Vec<NavigationItem> {
        self.children
            .iter()
            .map(|(name, node)| {
                let mut parts = ancestors.to_vec();
                parts.push(name.clone());
                let title = Some(format!(
                    "{prefix}{}",
                    if full { parts.join(".") } else { name.clone() }
                ));
                if node.children.is_empty() {
                    item(title, node.path.clone(), Vec::new())
                } else {
                    let mut children = Vec::new();
                    if let Some(path) = &node.path {
                        children.push(item(
                            None,
                            Some(path.clone()),
                            Vec::new(),
                        ));
                    }
                    children.extend(node.items(prefix, full, &parts));
                    item(title, None, children)
                }
            })
            .collect()
    }

    pub(super) fn summary(
        &self, depth: usize, root: &str, output: &mut String,
    ) {
        for (name, node) in &self.children {
            let indentation = "    ".repeat(depth);
            if let Some(path) = &node.path {
                let _ = writeln!(
                    output,
                    "{indentation}- [{name}]({})",
                    path.strip_prefix(&format!("{root}/")).unwrap_or(path)
                );
            } else {
                let _ = writeln!(output, "{indentation}- {name}");
            }
            node.summary(depth + 1, root, output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{item, replace_section, Node, Section};

    #[test]
    fn tree_preserves_module_order_and_indexes_before_children() {
        let mut tree = Node::default();
        tree.insert(&["z".into()], "api/z/index.md".into());
        tree.insert(&["a".into()], "api/a/index.md".into());
        tree.insert(&["z".into(), "child".into()], "api/z/child.md".into());
        let items = tree.items("MOD ", true, &[]);
        assert_eq!(items[0].title.as_deref(), Some("MOD z"));
        assert_eq!(items[1].title.as_deref(), Some("MOD a"));
        assert_eq!(items[0].children[0].url.as_deref(), Some("api/z/index.md"));
        assert_eq!(items[0].children[1].title.as_deref(), Some("MOD z.child"));
    }

    #[test]
    fn navigation_merges_existing_children_and_rejects_wrong_directory() {
        let section = Section {
            root: "api".into(),
            title: Some("API".into()),
            children: vec![],
            autonav: true,
            generated: true,
        };
        let mut items = vec![item(
            Some("API".into()),
            None,
            vec![item(None, Some("intro.md".into()), vec![])],
        )];
        let generated = vec![item(None, Some("api/pkg.md".into()), vec![])];
        assert!(replace_section(&mut items, &section, &generated).unwrap());
        assert_eq!(items[0].children.len(), 2);
        items[0].url = Some("wrong/".into());
        assert!(replace_section(&mut items, &section, &generated).is_err());
    }
}
