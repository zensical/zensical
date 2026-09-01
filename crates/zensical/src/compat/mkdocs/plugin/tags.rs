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

//! MkDocs Material tags compatibility pipeline.

use anyhow::{anyhow, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

use zrx::id::Id;
use zrx::scheduler::Value;
use zrx::stream::{Key, Stream, StreamTupleExt};

use crate::compat::mkdocs::html::{self, Editor, Visitor};
use crate::compat::mkdocs::plugin::search;
use crate::config::plugins::TagsPluginConfig;
use crate::config::{Config, Project};
use crate::path::SourcePath;
use crate::structure::page::Page;
use crate::structure::tag::{Tag as TemplateTag, TagLink};
use crate::structure::toc::Section;
use crate::template::Template;

mod listing;
mod parser;
mod render;
mod select;
mod tag;

pub use parser::Parser;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MkDocs Material tags compatibility pipeline.
#[derive(Clone, Debug)]
pub struct Tags {
    /// Enabled instances in configuration order.
    instances: Arc<[Instance]>,
    /// Template-visible project configuration shared by fragments.
    project: Arc<Project>,
}

// ----------------------------------------------------------------------------

/// Inputs required to derive tags patches.
pub struct Dependencies<'a> {
    /// Rendered pages and their page-local tags facts.
    pub pages: &'a Stream<Id, PageInput>,
}

// ----------------------------------------------------------------------------

/// One configured tags plugin instance.
#[derive(Clone, Debug)]
struct Instance {
    /// Stable configuration-order identity.
    id: usize,
    /// Public MkDocs plugin instance name.
    name: String,
    /// Fully normalized compatibility configuration.
    config: Arc<TagsPluginConfig>,
    /// Compiled source admission patterns.
    filter: SourceFilter,
}

// ----------------------------------------------------------------------------

/// Compiled include-then-exclude source filter.
#[derive(Clone, Debug)]
struct SourceFilter {
    /// Sources admitted before exclusions are applied.
    include: GlobSet,
    /// Sources removed after inclusion.
    exclude: GlobSet,
    /// Whether inclusion is restricted by at least one pattern.
    has_include: bool,
    /// Deferred pattern compilation failure.
    error: Option<String>,
}

// ----------------------------------------------------------------------------

/// Cached page-local facts produced during the shared HTML pass.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Facts {
    /// Per-instance normalized mappings for this page.
    mappings: Vec<listing::Mapping>,
    /// Listing directives in source order.
    listings: Vec<listing::Prepared>,
    /// Whether final HTML must remove a retained search directive.
    search_cleanup: bool,
}

// ----------------------------------------------------------------------------

/// Page-local input relation consumed by the tags pipeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageInput {
    /// Rendered page shared with downstream workflow branches.
    pub page: Page,
    /// Cached mapping and listing facts from the shared HTML pass.
    pub facts: Arc<Facts>,
}

// ----------------------------------------------------------------------------

/// Derived page patch emitted for every live page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    /// HTML after owned listing slots or deferred directives changed it.
    pub content: Option<String>,
    /// TOC after owned listing subtrees have been inserted.
    pub toc: Option<Vec<Section>>,
    /// Page-level template variables keyed by configured names.
    pub variables: BTreeMap<String, Vec<TemplateTag>>,
    /// Refreshed search facts when listing HTML changed.
    pub search: Option<Arc<search::Facts>>,
}

// ----------------------------------------------------------------------------

/// Exact replacements applied to generated listing markers in one HTML pass.
struct SlotPatcher<'a> {
    /// Marker and rendered-fragment pairs.
    replacements: Vec<(&'a str, &'a str)>,
    /// Number of markers replaced during the pass.
    replaced: usize,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Tags {
    /// Resolves the private settings owned by this pipeline instance.
    pub fn new(config: &Config, serve: bool) -> Self {
        let instances = config
            .project
            .plugins
            .tags
            .config
            .iter()
            .enumerate()
            .map(|(id, plugin)| {
                let mut config = plugin.config.clone();
                if serve && config.shadow_on_serve {
                    config.shadow = true;
                }
                Instance {
                    id,
                    name: plugin.name.clone(),
                    filter: SourceFilter::new(
                        &config.filters.include,
                        &config.filters.exclude,
                    ),
                    config: Arc::new(config),
                }
            })
            .filter(|instance| instance.config.enabled)
            .collect::<Vec<_>>();
        Self {
            instances: instances.into(),
            project: config.project.clone(),
        }
    }

    /// Returns whether no tags instance participates in page processing.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Derives listing and page patches from revision-complete selections.
    pub fn setup(&self, dependencies: Dependencies<'_>) -> Stream<Id, Patch> {
        let pages = dependencies.pages;
        let mappings = pages.map(PageInput::mapping);
        let listings = pages.flat_map(|input: &PageInput| {
            input
                .facts
                .listings
                .iter()
                .map(|prepared| {
                    (
                        listing_key(prepared.instance, prepared.ordinal),
                        listing::Listing {
                            prepared: prepared.clone(),
                            page: input.page.clone(),
                        },
                    )
                })
                .collect::<Vec<_>>()
        });

        // Materialize one bounded member set for every live listing.
        let members =
            mappings.select(&listings, |listing: &listing::Listing| {
                let listing = listing.clone();
                move |mapping: &listing::PageMapping| listing.matches(mapping)
            });
        let tags = self.clone();
        let template = Template::new(self.project.theme_dirs.clone());
        let rendered = (listings, members).join().map(
            move |(listing, members): &(
                listing::Listing,
                Vec<(Key<Id>, listing::PageMapping)>,
            )| {
                render::listing(
                    select::Selection {
                        configuration: listing.clone(),
                        members: members.clone(),
                    },
                    &tags,
                    &template,
                )
            },
        );

        // Materialize the rendered listings that can affect each live page.
        let members = rendered.select(pages, |input: &PageInput| {
            let input = input.clone();
            move |rendered: &render::Rendered| affects(&input, rendered)
        });
        let tags = self.clone();
        (pages.clone(), members).join().map(
            move |(input, members): &(
                PageInput,
                Vec<(Key<Id>, render::Rendered)>,
            )| {
                patch(
                    select::Selection {
                        configuration: input.clone(),
                        members: members.clone(),
                    },
                    &tags,
                )
            },
        )
    }
}

// ----------------------------------------------------------------------------

impl PageInput {
    /// Projects this page into the relation consumed by listing selection.
    fn mapping(&self) -> listing::PageMapping {
        listing::PageMapping {
            page: self.page.clone(),
            mappings: self.facts.mappings.clone(),
        }
    }
}

// ----------------------------------------------------------------------------

impl Facts {
    /// Records cleanup deferred until listings have patched final page HTML.
    pub fn require_search_cleanup(&mut self) {
        self.search_cleanup = true;
    }
}

// ----------------------------------------------------------------------------

impl SourceFilter {
    /// Compiles source patterns once for the workflow lifetime.
    fn new(include: &[String], exclude: &[String]) -> Self {
        let mut error = None;
        let include_set = compile_globs(include, &mut error);
        let exclude_set = compile_globs(exclude, &mut error);
        Self {
            include: include_set,
            exclude: exclude_set,
            has_include: !include.is_empty(),
            error,
        }
    }

    /// Applies Material's inclusion-first, exclusion-second semantics.
    fn accepts(&self, source: &SourcePath) -> Result<bool> {
        if let Some(error) = &self.error {
            return Err(anyhow!("invalid tags source filter: {error}"));
        }
        let source = source.as_str();
        Ok((!self.has_include || self.include.is_match(source))
            && !self.exclude.is_match(source))
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Visitor for SlotPatcher<'_> {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        if !matches!(event, CallbackEvent::Comment { .. }) {
            return;
        }
        let source = editor.text(span.start..span.end);
        let replacement = self
            .replacements
            .iter()
            .find(|(slot, _)| *slot == source)
            .map(|(_, replacement)| *replacement);
        if let Some(replacement) = replacement {
            editor.replace(span.start..span.end, replacement);
            self.replaced += 1;
        }
    }
}

// ----------------------------------------------------------------------------

impl Value for PageInput {}

// ----------------------------------------------------------------------------

impl Value for Patch {}

// ----------------------------------------------------------------------------

impl Value for listing::Listing {}

// ----------------------------------------------------------------------------

impl Value for listing::PageMapping {}

// ----------------------------------------------------------------------------

impl Value for render::Rendered {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Compiles fnmatch-like patterns whose wildcards may cross path separators.
fn compile_globs(patterns: &[String], error: &mut Option<String>) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match GlobBuilder::new(pattern)
            .literal_separator(false)
            .backslash_escape(false)
            .build()
        {
            Ok(pattern) => {
                builder.add(pattern);
            }
            Err(reason) => {
                error.get_or_insert_with(|| reason.to_string());
            }
        }
    }
    builder.build().unwrap_or_else(|reason| {
        error.get_or_insert_with(|| reason.to_string());
        GlobSetBuilder::new().build().expect("empty glob set")
    })
}

/// Creates a page-local suffix key for one listing ordinal.
fn listing_key(instance: usize, ordinal: u32) -> Key<Id> {
    Key::from(
        Id::builder()
            .provider("tags")
            .context(instance.to_string())
            .location(ordinal.to_string())
            .build()
            .expect("numeric tags listing identity is valid"),
    )
}

/// Returns whether one rendered listing can affect one page.
fn affects(input: &PageInput, rendered: &render::Rendered) -> bool {
    if rendered.listing.page.source() == input.page.source() {
        return true;
    }
    let mapping = input.mapping();
    rendered
        .listing
        .selected_tags(&mapping)
        .into_iter()
        .any(|tag| {
            rendered.targets.iter().any(|target| {
                target.instance == rendered.listing.prepared.instance
                    && target.name == tag.name
            })
        })
}

/// Builds one coherent page patch from revision-complete rendered listings.
fn patch(
    selection: select::Selection<PageInput, render::Rendered>, tags: &Tags,
) -> anyhow::Result<Patch> {
    let input = selection.configuration;
    let mut rendered = selection
        .members
        .into_iter()
        .map(|(_, rendered)| rendered)
        .collect::<Vec<_>>();
    rendered.sort_by(|left, right| {
        left.listing
            .page
            .source()
            .cmp(right.listing.page.source())
            .then(
                left.listing
                    .prepared
                    .ordinal
                    .cmp(&right.listing.prepared.ordinal),
            )
    });

    let owned = rendered
        .iter()
        .filter(|listing| listing.listing.page.source() == input.page.source())
        .collect::<Vec<_>>();
    let changed = !owned.is_empty();
    let mut content = (changed || input.facts.search_cleanup)
        .then(|| input.page.content.clone());
    let mut toc = changed.then(|| input.page.toc.clone());
    if changed {
        let html = content.as_mut().expect("owned listings require HTML");
        let mut patcher = SlotPatcher {
            replacements: owned
                .iter()
                .map(|listing| {
                    (
                        listing.listing.prepared.slot.as_str(),
                        listing.html.as_str(),
                    )
                })
                .collect(),
            replaced: 0,
        };
        let expected = patcher.replacements.len();
        *html = html::scan(html, &mut [&mut patcher]).ok_or_else(|| {
            anyhow!("generated tags listing markers are missing from page HTML")
        })?;
        if patcher.replaced != expected {
            return Err(anyhow!(
                "replaced {} of {expected} generated tags listing markers",
                patcher.replaced
            ));
        }
    }
    for listing in owned {
        insert_toc(
            toc.as_mut().expect("owned listings require a TOC"),
            listing,
        );
    }

    let variables = derive_variables(&input, &rendered, tags)?;
    let search = if (changed || input.facts.search_cleanup)
        && tags.project.plugins.search.config.enabled
    {
        let mut parser = search::parser(&input.page.meta);
        let html = content
            .as_mut()
            .expect("search refresh requires final page HTML");
        if let Some(cleaned) = html::scan(html, &mut [&mut parser]) {
            *html = cleaned;
        }
        Some(search::finish(parser))
    } else {
        None
    };
    Ok(Patch {
        content,
        toc,
        variables,
        search,
    })
}

/// Derives template-visible tag references and their nearest listing links.
fn derive_variables(
    input: &PageInput, rendered: &[render::Rendered], tags: &Tags,
) -> Result<BTreeMap<String, Vec<TemplateTag>>> {
    let mapping = input.mapping();
    let mut variables = Vec::new();
    for instance in tags
        .instances
        .iter()
        .filter(|instance| instance.config.tags)
    {
        let Some(tags) = mapping
            .mappings
            .iter()
            .find(|mapping| mapping.instance == instance.id)
        else {
            continue;
        };
        let mut references = Vec::with_capacity(tags.tags.len());
        for tag in &tags.tags {
            let template = tag.template();
            let mut links = rendered
                .iter()
                .filter_map(|listing| {
                    tag_link(listing, &mapping, instance.id, &tag.name)
                })
                .collect::<Vec<_>>();
            links.sort_by(|left, right| {
                right.0.cmp(&left.0).then(left.1.url.cmp(&right.1.url))
            });
            let links =
                links.into_iter().map(|(_, link)| link).collect::<Vec<_>>();
            references.push(TemplateTag {
                name: template.name,
                parent: template.parent.map(|parent| *parent),
                url: links.first().map(|link| link.url.clone()),
                hidden: template.hidden,
                links,
            });
        }
        tag::sort_references(&mut references, &instance.config)?;
        variables
            .push((instance.config.tags_name_variable.clone(), references));
    }
    Ok(tag::variables(variables))
}

/// Resolves one page tag to a rendered listing target, if it participates.
fn tag_link(
    rendered: &render::Rendered, mapping: &listing::PageMapping,
    instance: usize, name: &str,
) -> Option<(usize, TagLink)> {
    if rendered.listing.prepared.instance != instance
        || !rendered
            .listing
            .selected_tags(mapping)
            .iter()
            .any(|selected| selected.name == name)
    {
        return None;
    }
    let target = rendered.targets.iter().find(|target| target.name == name)?;
    let base = if rendered.listing.page.url.is_empty() {
        "."
    } else {
        &rendered.listing.page.url
    };
    Some((
        closeness(&mapping.page.url, &rendered.listing.page.url),
        TagLink {
            title: rendered.listing.page.title.clone(),
            url: format!("{base}#{}", target.slug),
        },
    ))
}

/// Inserts one listing TOC subtree below its recorded heading host.
fn insert_toc(toc: &mut Vec<Section>, rendered: &render::Rendered) {
    if rendered.toc.is_empty() {
        return;
    }
    let prepared = &rendered.listing.prepared;
    let children = match &prepared.host {
        Some(host) => {
            find_section_mut(toc, host).map(|section| &mut section.children)
        }
        None => Some(toc),
    };
    let Some(children) = children else {
        return;
    };
    let index = prepared
        .following
        .as_ref()
        .and_then(|following| {
            children.iter().position(|section| &section.id == following)
        })
        .unwrap_or(children.len());
    children.splice(index..index, rendered.toc.clone());
}

/// Finds one section recursively by its stable heading ID.
fn find_section_mut<'a>(
    sections: &'a mut [Section], id: &str,
) -> Option<&'a mut Section> {
    for section in sections {
        if section.id == id {
            return Some(section);
        }
        if let Some(section) = find_section_mut(&mut section.children, id) {
            return Some(section);
        }
    }
    None
}

/// Computes Material's path-closeness ordering without filesystem access.
fn closeness(left: &str, right: &str) -> usize {
    let common = left
        .split('/')
        .zip(right.split('/'))
        .take_while(|(left, right)| left == right)
        .map(|(component, _)| component.len() + 1)
        .sum::<usize>();
    common.saturating_sub(1)
}
