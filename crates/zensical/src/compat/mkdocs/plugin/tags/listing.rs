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

//! Listing configuration, membership, and tree construction.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::config::plugins::{TagsListingConfig, TagsPluginConfig};
use crate::structure::page::Page;
use crate::structure::tag::TagNode as TemplateTagNode;

use super::tag::{Tag, TagNode};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Fully resolved configuration for one listing directive.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Config {
    /// Whether mappings are restricted to the listing directory.
    pub scope: bool,
    /// Whether hidden tags are rendered.
    pub shadow: bool,
    /// Fragment layout name.
    pub layout: String,
    /// Whether tag nodes are added to the page table of contents.
    pub toc: bool,
    /// Included tag names.
    pub include: BTreeSet<String>,
    /// Excluded tag names.
    pub exclude: BTreeSet<String>,
}

// ----------------------------------------------------------------------------

/// One listing discovered in a rendered Markdown page.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Prepared {
    /// Plugin instance index.
    pub instance: usize,
    /// Page-local source ordinal.
    pub ordinal: u32,
    /// Unambiguous HTML replacement slot.
    pub slot: String,
    /// Nearest owning heading identifier, or root when absent.
    pub host: Option<String>,
    /// Level of the owning heading or the synthetic root.
    pub host_level: u8,
    /// First following child heading used to retain source ordering.
    pub following: Option<String>,
    /// Resolved listing configuration.
    pub config: Config,
}

// ----------------------------------------------------------------------------

/// Listing enriched with its owner page facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    /// Prepared page-local listing facts.
    pub prepared: Prepared,
    /// Shared owner page context used by fragment rendering.
    pub page: Page,
}

// ----------------------------------------------------------------------------

/// One page mapping for one plugin instance.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Mapping {
    /// Plugin instance index.
    pub instance: usize,
    /// Normalized leaf tags.
    pub tags: Vec<Tag>,
}

// ----------------------------------------------------------------------------

/// Page facts consumed by listing membership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageMapping {
    /// Complete page object exposed to listing fragments.
    pub page: Page,
    /// Per-instance mapping facts.
    pub mappings: Vec<Mapping>,
}

// ----------------------------------------------------------------------------

impl PageMapping {
    /// Returns the normalized tags owned by one configured plugin instance.
    fn tags(&self, instance: usize) -> Option<&[Tag]> {
        self.mappings
            .iter()
            .find(|mapping| mapping.instance == instance)
            .map(|mapping| mapping.tags.as_slice())
    }
}

// ----------------------------------------------------------------------------

/// One hierarchical listing node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tree {
    /// Cumulative tag represented by this node.
    pub tag: TagNode,
    /// Pages carrying this exact leaf tag.
    pub mappings: Vec<TemplateMapping>,
    /// Child tag nodes.
    pub children: Vec<Tree>,
    /// Rendered heading content assigned by the renderer.
    pub content: String,
}

// ----------------------------------------------------------------------------

/// MkDocs-compatible mapping exposed to listing fragments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TemplateMapping {
    /// MkDocs-compatible page nested below the mapping.
    pub item: Page,
    /// Complete normalized leaf tags carried by the page mapping.
    pub tags: Vec<TemplateTagNode>,
}

// ----------------------------------------------------------------------------

/// Mutable construction node converted into the public ordered tree.
struct Node {
    /// Cumulative tag represented by this node.
    tag: TagNode,
    /// Pages carrying this exact leaf tag, keyed by URL.
    mappings: BTreeMap<String, TemplateMapping>,
    /// Child nodes keyed by cumulative tag name.
    children: BTreeMap<String, Node>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Config {
    /// Resolves optional per-listing values against plugin defaults.
    pub fn new(value: &TagsListingConfig, plugin: &TagsPluginConfig) -> Self {
        Self {
            scope: value.scope.unwrap_or(false),
            shadow: value.shadow.unwrap_or(plugin.shadow),
            layout: value
                .layout
                .clone()
                .unwrap_or_else(|| plugin.listings_layout.clone()),
            toc: value.toc.unwrap_or(plugin.listings_toc),
            include: value
                .include
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            exclude: value
                .exclude
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        }
    }

    /// Creates the default listing configuration.
    pub fn default_for(plugin: &TagsPluginConfig) -> Self {
        Self::new(
            &TagsListingConfig {
                scope: None,
                shadow: None,
                layout: None,
                toc: None,
                include: None,
                exclude: None,
            },
            plugin,
        )
    }
}

// ----------------------------------------------------------------------------

impl Listing {
    /// Returns the leaf tags from one mapping included by this listing.
    pub fn selected_tags<'a>(&self, mapping: &'a PageMapping) -> Vec<&'a Tag> {
        if mapping.page.source() == self.page.source() {
            return Vec::new();
        }
        if self.prepared.config.scope {
            let inside = self.page.source().parent().is_none_or(|parent| {
                mapping.page.source().is_descendant_of(&parent)
            });
            if !inside {
                return Vec::new();
            }
        }
        let Some(tags) = mapping.tags(self.prepared.instance) else {
            return Vec::new();
        };
        if tags
            .iter()
            .any(|tag| tag.contains(&self.prepared.config.exclude))
        {
            return Vec::new();
        }
        tags.iter()
            .filter(|tag| {
                self.prepared.config.include.is_empty()
                    || tag.contains(&self.prepared.config.include)
            })
            .collect()
    }

    /// Returns whether a page contributes at least one tag to this listing.
    pub fn matches(&self, mapping: &PageMapping) -> bool {
        !self.selected_tags(mapping).is_empty()
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Builds and deterministically sorts the listing tree.
pub fn tree(
    listing: &Listing, mappings: impl IntoIterator<Item = PageMapping>,
    plugin: &TagsPluginConfig,
) -> Result<Vec<Tree>> {
    let mut roots = BTreeMap::<String, Node>::new();
    for mapping in mappings {
        let tags = mapping
            .tags(listing.prepared.instance)
            .into_iter()
            .flatten()
            .map(Tag::template)
            .collect();
        let item = TemplateMapping {
            item: mapping.page.clone(),
            tags,
        };
        for tag in listing.selected_tags(&mapping) {
            if tag.hidden() && !listing.prepared.config.shadow {
                continue;
            }
            insert(&mut roots, &tag.hierarchy, item.clone());
        }
    }
    finalize(roots, plugin)
}

/// Inserts one leaf and its ancestors into the mutable tree.
fn insert(
    roots: &mut BTreeMap<String, Node>, hierarchy: &[TagNode],
    item: TemplateMapping,
) {
    let mut level = roots;
    for (index, tag) in hierarchy.iter().enumerate() {
        let node = level.entry(tag.name.clone()).or_insert_with(|| Node {
            tag: tag.clone(),
            mappings: BTreeMap::new(),
            children: BTreeMap::new(),
        });
        if index + 1 == hierarchy.len() {
            node.mappings.insert(item.item.url.clone(), item.clone());
        }
        level = &mut node.children;
    }
}

/// Converts and sorts every level of the mutable tree.
fn finalize(
    nodes: BTreeMap<String, Node>, config: &TagsPluginConfig,
) -> Result<Vec<Tree>> {
    let mut output = nodes
        .into_values()
        .map(|node| {
            let mut mappings = node.mappings.into_values().collect::<Vec<_>>();
            match config.listings_sort_by.as_str() {
                "item_title" => mappings.sort_by(|left, right| {
                    left.item
                        .title
                        .cmp(&right.item.title)
                        .then(left.item.url.cmp(&right.item.url))
                }),
                "item_url" => mappings
                    .sort_by(|left, right| left.item.url.cmp(&right.item.url)),
                strategy => {
                    bail!("unsupported listing sort strategy: {strategy}")
                }
            }
            if config.listings_sort_reverse {
                mappings.reverse();
            }
            Ok(Tree {
                tag: node.tag,
                mappings,
                children: finalize(node.children, config)?,
                content: String::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    match config.listings_tags_sort_by.as_str() {
        "tag_name" => {
            output.sort_by(|left, right| left.tag.name.cmp(&right.tag.name));
        }
        "tag_name_casefold" => output
            .sort_by_cached_key(|tree| super::tag::casefold(&tree.tag.name)),
        strategy => bail!("unsupported listing tag sort strategy: {strategy}"),
    }
    if config.listings_tags_sort_reverse {
        output.reverse();
    }
    Ok(output)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::Config;

    #[test]
    fn include_and_exclude_are_exact_sets() {
        let config = Config {
            scope: false,
            shadow: false,
            layout: "default".into(),
            toc: true,
            include: ["A".into()].into_iter().collect(),
            exclude: ["B".into()].into_iter().collect(),
        };
        assert_eq!(config.include, BTreeSet::from(["A".into()]));
        assert_eq!(config.exclude, BTreeSet::from(["B".into()]));
    }
}
