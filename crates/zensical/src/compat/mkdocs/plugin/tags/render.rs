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

//! Listing fragment rendering and coherent derived facts.

use anyhow::{anyhow, Result};
use minijinja::value::{Enumerator, Object, ObjectExt, ObjectRepr};
use minijinja::{context, Value};
use std::fmt;
use std::sync::Arc;

use crate::config::relative_base_url;
use crate::config::Project;
use crate::structure::toc::Section;
use crate::template::Template;

use super::listing::{self, Listing, PageMapping, Tree};
use super::select::Selection;
use super::Tags;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Native fallback for Material's default tag fragment.
const TAG_TEMPLATE: &str = r#"{%- set class = "md-tag" -%}
{%- if tag.hidden %}{% set class = class ~ " md-tag-shadow" %}{% endif -%}
{%- if config.extra.tags -%}
  {%- set class = class ~ " md-tag-icon" -%}
  {%- if tag.name in config.extra.tags -%}
    {%- set class = class ~ " md-tag--" ~ config.extra.tags[tag.name] -%}
  {%- endif -%}
{%- endif -%}
<span class="{{ class }}">{{ tag.name }}</span>"#;

/// Native fallback for Material's default recursive listing fragment.
const LISTING_TEMPLATE: &str = r#"{% macro render(listing) %}
  {{ listing.content }}
  <ul>
    {% for mapping in listing.mappings %}
      <li>
        <a href="{{ mapping.item.url | url }}">
          {{ mapping.item.title }}
        </a>
      </li>
    {% endfor %}
    {% for child in listing %}
      <li style="list-style-type:none">{{ render(child) }}</li>
    {% endfor %}
  </ul>
{% endmacro %}
{{ render(listing) }}"#;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One rendered listing and every derived fact from the same tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    /// Source listing configuration.
    pub listing: Listing,
    /// Rendered listing HTML.
    pub html: String,
    /// Optional table-of-contents subtree.
    pub toc: Vec<Section>,
    /// Public tag anchors emitted by the rendered tree.
    pub targets: Vec<Target>,
}

// ----------------------------------------------------------------------------

/// Public anchor emitted for one listing tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Plugin instance index.
    pub instance: usize,
    /// Cumulative tag name.
    pub name: String,
    /// Listing fragment.
    pub slug: String,
    /// Whether the tag is hidden.
    pub hidden: bool,
}

// ----------------------------------------------------------------------------

/// MiniJinja view preserving Material's iterable listing-tree contract.
#[derive(Clone)]
struct TreeView(Tree);

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl fmt::Debug for TreeView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("TreeView")
            .field(&self.0.tag.name)
            .finish()
    }
}

impl Object for TreeView {
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        ObjectRepr::Iterable
    }

    fn get_value_by_str(self: &Arc<Self>, key: &str) -> Option<Value> {
        match key {
            "tag" => Some(Value::from_serialize(&self.0.tag)),
            "content" => Some(Value::from(self.0.content.clone())),
            "mappings" => Some(Value::from_serialize(&self.0.mappings)),
            "children" => Some(Value::from_serialize(&self.0.children)),
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        self.mapped_enumerator(|this| {
            Box::new(
                this.0
                    .children
                    .iter()
                    .cloned()
                    .map(TreeView)
                    .map(Value::from_object),
            )
        })
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Renders one revision-complete listing selection.
pub fn listing(
    selection: Selection<Listing, PageMapping>, tags: &Tags,
    template: &Template<'_>,
) -> Result<Rendered> {
    let listing = selection.configuration;
    let instance = tags
        .instances
        .iter()
        .find(|instance| instance.id == listing.prepared.instance)
        .ok_or_else(|| {
            anyhow!("tags listing references a missing plugin instance")
        })?;
    let mappings = selection
        .members
        .into_iter()
        .map(|(_, mapping)| mapping)
        .collect::<Vec<_>>();
    let mut trees = listing::tree(&listing, mappings, &instance.config)?;
    prepare_tree(
        &mut trees,
        listing.prepared.host_level + 1,
        &listing,
        &tags.project,
        template,
    )?;

    let layout = &listing.prepared.config.layout;
    let name = format!("fragments/tags/{layout}/listing.html");
    let base_url = relative_base_url(&listing.page.url);
    let mut html = Vec::with_capacity(trees.len());
    for tree in &trees {
        html.push(template.render_fragment(
            &name,
            LISTING_TEMPLATE,
            context! {
                config => &tags.project,
                page => &listing.page,
                base_url => &base_url,
                listing => Value::from_object(TreeView(tree.clone())),
            },
        )?);
    }
    let toc = if listing.prepared.config.toc {
        trees.iter().map(section).collect()
    } else {
        Vec::new()
    };
    let mut targets = Vec::new();
    collect_targets(&trees, listing.prepared.instance, &mut targets);
    Ok(Rendered {
        listing,
        html: html.join("\n"),
        toc,
        targets,
    })
}

/// Renders tag headings and recursively prepares child nodes.
fn prepare_tree(
    trees: &mut [Tree], level: u8, listing: &Listing, project: &Arc<Project>,
    template: &Template<'_>,
) -> Result<()> {
    let level = level.min(6);
    let name =
        format!("fragments/tags/{}/tag.html", listing.prepared.config.layout);
    let base_url = relative_base_url(&listing.page.url);
    for tree in trees {
        let tag = template.render_fragment(
            &name,
            TAG_TEMPLATE,
            context! {
                config => project,
                page => &listing.page,
                base_url => &base_url,
                tag => &tree.tag,
            },
        )?;
        tree.content =
            format!("<h{level} id=\"{}\">{tag}</h{level}>", tree.tag.slug);
        prepare_tree(
            &mut tree.children,
            level.saturating_add(1).min(6),
            listing,
            project,
            template,
        )?;
    }
    Ok(())
}

/// Converts one rendered tree into the common table-of-contents model.
fn section(tree: &Tree) -> Section {
    let level = heading_level(&tree.content).unwrap_or(2);
    Section {
        title: tree.tag.name.clone(),
        content: tree.tag.name.clone(),
        id: tree.tag.slug.clone(),
        url: format!("#{}", tree.tag.slug),
        children: tree.children.iter().map(section).collect(),
        level,
    }
}

/// Reads the level from the generated opening heading.
fn heading_level(content: &str) -> Option<u8> {
    content
        .as_bytes()
        .get(2)
        .and_then(|byte| byte.is_ascii_digit().then_some(byte - b'0'))
}

/// Collects every public anchor once from the listing tree.
fn collect_targets(trees: &[Tree], instance: usize, output: &mut Vec<Target>) {
    for tree in trees {
        output.push(Target {
            instance,
            name: tree.tag.name.clone(),
            slug: tree.tag.slug.clone(),
            hidden: tree.tag.hidden,
        });
        collect_targets(&tree.children, instance, output);
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::heading_level;

    #[test]
    fn reads_generated_heading_levels() {
        assert_eq!(heading_level("<h3 id=\"tag:x\">X</h3>"), Some(3));
    }
}
