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

//! Structured post-link parsing and revision-complete page resolution.

use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::compat::mkdocs::resource::Resource;
use crate::path::SourcePath;
use crate::structure::dynamic::Dynamic;
use crate::structure::nav::NavigationItem;
use crate::structure::page::Page;
use crate::structure::toc::Section;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// One navigation-shaped post-link item.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkItem {
    /// Page, asset, or external URL.
    Reference {
        /// Optional explicit display title.
        title: Option<String>,
        /// Unresolved link target from post metadata.
        target: String,
    },
    /// Named nested group.
    Section {
        /// Display title of the group.
        title: String,
        /// Nested references and sections in declaration order.
        children: Vec<LinkItem>,
    },
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Revision-complete targets shared by every post in one blog view.
pub struct Resolver<'a> {
    /// Rendered pages indexed by physical source identity.
    pages: HashMap<SourcePath, &'a Page>,
    /// Emitted resources indexed by physical source path.
    resources: HashMap<&'a str, &'a Resource>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Resolver<'a> {
    /// Builds one lookup index for all post link resolutions in a view.
    pub fn new(
        pages: impl IntoIterator<Item = &'a Page>,
        resources: impl IntoIterator<Item = &'a Resource>,
    ) -> Self {
        Self {
            pages: pages
                .into_iter()
                .map(|page| (page.source().clone(), page))
                .collect(),
            resources: resources
                .into_iter()
                .map(|resource| (resource.source_path.as_str(), resource))
                .collect(),
        }
    }

    /// Resolves page and asset facts while preserving missing/external links.
    pub fn resolve(&self, items: &[LinkItem]) -> anyhow::Result<Dynamic> {
        let items = items
            .iter()
            .map(|item| self.resolve_item(item))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Dynamic::List(
            items
                .iter()
                .map(Dynamic::from_serialize)
                .collect::<Result<_, _>>()?,
        ))
    }

    fn resolve_item(&self, item: &LinkItem) -> anyhow::Result<NavigationItem> {
        resolve_item(item, &self.pages, &self.resources)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Parses optional navigation-shaped `links` metadata.
pub fn parse(value: Option<&Dynamic>) -> anyhow::Result<Option<Vec<LinkItem>>> {
    let Some(value) = value else { return Ok(None) };
    let Dynamic::List(items) = value else {
        bail!("post links must be a list")
    };
    items
        .iter()
        .map(parse_item)
        .collect::<anyhow::Result<_>>()
        .map(Some)
}

/// Returns local page sources referenced by a structured link tree.
pub fn targets(items: &[LinkItem]) -> HashSet<SourcePath> {
    let mut targets = HashSet::new();
    collect_targets(items, &mut targets);
    targets
}

fn parse_item(value: &Dynamic) -> anyhow::Result<LinkItem> {
    match value {
        Dynamic::String(target) => Ok(LinkItem::Reference {
            title: None,
            target: target.clone(),
        }),
        Dynamic::Map(values) if values.len() == 1 => {
            let (title, value) = values.iter().next().expect("checked length");
            match value {
                Dynamic::String(target) => Ok(LinkItem::Reference {
                    title: Some(title.clone()),
                    target: target.clone(),
                }),
                Dynamic::List(children) => Ok(LinkItem::Section {
                    title: title.clone(),
                    children: children
                        .iter()
                        .map(parse_item)
                        .collect::<anyhow::Result<_>>()?,
                }),
                _ => bail!("post link '{title}' must target a URL or list"),
            }
        }
        Dynamic::Map(_) => bail!("post link mappings must contain one item"),
        _ => bail!("post link items must be URLs or one-item mappings"),
    }
}

fn collect_targets(items: &[LinkItem], targets: &mut HashSet<SourcePath>) {
    for item in items {
        match item {
            LinkItem::Reference { target, .. } if is_local(target) => {
                let path = target
                    .split_once('#')
                    .map_or(target.as_str(), |item| item.0);
                if let Ok(path) = path.trim_start_matches('/').parse() {
                    targets.insert(path);
                }
            }
            LinkItem::Section { children, .. } => {
                collect_targets(children, targets);
            }
            LinkItem::Reference { .. } => {}
        }
    }
}

fn resolve_item(
    item: &LinkItem, pages: &HashMap<SourcePath, &Page>,
    resources: &HashMap<&str, &Resource>,
) -> anyhow::Result<NavigationItem> {
    match item {
        LinkItem::Section { title, children } => Ok(NavigationItem {
            title: Some(title.clone()),
            url: None,
            canonical_url: None,
            meta: None,
            children: children
                .iter()
                .map(|item| resolve_item(item, pages, resources))
                .collect::<anyhow::Result<_>>()?,
            is_index: false,
            active: false,
        }),
        LinkItem::Reference { title, target } => {
            let (path, fragment) = target
                .split_once('#')
                .map_or((target.as_str(), None), |(path, fragment)| {
                    (path, Some(fragment))
                });
            let source = path.trim_start_matches('/').parse::<SourcePath>();
            let Some(page) = source.ok().and_then(|source| pages.get(&source))
            else {
                let resource = resources.get(path).copied();
                return Ok(NavigationItem {
                    title: title.clone(),
                    url: Some(resource.map_or_else(
                        || target.clone(),
                        |resource| {
                            fragment.map_or_else(
                                || resource.path.to_string(),
                                |fragment| {
                                    format!("{}#{fragment}", resource.path)
                                },
                            )
                        },
                    )),
                    canonical_url: None,
                    meta: None,
                    children: Vec::new(),
                    is_index: false,
                    active: false,
                });
            };
            let mut meta = page.meta.clone();
            let url = match fragment {
                None => page.url.clone(),
                Some(fragment) => {
                    if let Some(anchor) = find_anchor(&page.toc, fragment) {
                        meta.insert(
                            "subtitle".into(),
                            Dynamic::String(anchor.title.clone()),
                        );
                        format!("{}#{}", page.url, anchor.id)
                    } else {
                        target.clone()
                    }
                }
            };
            Ok(NavigationItem {
                title: title.clone().or_else(|| Some(page.title.clone())),
                url: Some(url),
                canonical_url: page.canonical_url.clone(),
                meta: Some(meta),
                children: Vec::new(),
                is_index: matches!(
                    page.source().file_name(),
                    "index.md" | "README.md"
                ),
                active: false,
            })
        }
    }
}

fn find_anchor<'a>(sections: &'a [Section], id: &str) -> Option<&'a Section> {
    sections.iter().find_map(|section| {
        (section.id == id)
            .then_some(section)
            .or_else(|| find_anchor(&section.children, id))
    })
}

fn is_local(target: &str) -> bool {
    !target.starts_with(['#', '?'])
        && !target
            .split('/')
            .next()
            .is_some_and(|prefix| prefix.contains(':'))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{parse, LinkItem};
    use crate::structure::dynamic::Dynamic;
    use std::collections::BTreeMap;

    #[test]
    fn parses_navigation_shaped_links() {
        let value = Dynamic::List(vec![
            Dynamic::String("guide.md".into()),
            Dynamic::Map(BTreeMap::from([(
                "References".into(),
                Dynamic::List(vec![Dynamic::Map(BTreeMap::from([(
                    "API".into(),
                    Dynamic::String("api.md#call".into()),
                )]))]),
            )])),
        ]);
        let links = parse(Some(&value)).unwrap().unwrap();
        assert_eq!(links.len(), 2);
        assert!(matches!(links[1], LinkItem::Section { .. }));
    }
}
