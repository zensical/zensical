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

//! Streaming discovery of tag mappings and listing directives.

use anyhow::{anyhow, bail, Context, Result};
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use saphyr::{LoadableYamlNode, Yaml};
use std::collections::BTreeMap;
use std::mem;

use crate::compat::mkdocs::html::{Editor, Visitor};
use crate::config::plugins::{
    python_bool, python_float, TagsListingConfig, TagsPluginConfig,
};
use crate::path::SourcePath;
use crate::structure::dynamic::Dynamic;

use super::listing::{Config as ListingConfig, Mapping, Prepared};
use super::tag;
use super::{Facts, Instance, Tags};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Page-local tags visitor.
pub struct Parser {
    /// Source currently being scanned.
    source: SourcePath,
    /// Enabled plugin instances that accept this source.
    instances: Vec<Instance>,
    /// Per-instance mappings derived from page metadata.
    mappings: Vec<Mapping>,
    /// Listing directives discovered in source order.
    listings: Vec<Prepared>,
    /// Open heading ancestry.
    headings: Vec<Heading>,
    /// Start tag currently receiving heading attributes.
    pending: Option<PendingHeading>,
    /// Listings awaiting their first following heading.
    waiting: Vec<usize>,
    /// First failure deferred until the shared HTML pass completes.
    error: Option<anyhow::Error>,
}

// ----------------------------------------------------------------------------

/// Heading currently receiving attributes.
struct PendingHeading {
    /// Parsed heading level.
    level: u8,
    /// Decoded heading identifier, when present.
    id: Option<String>,
    /// Whether the current attribute is the identifier.
    attribute_is_id: bool,
}

// ----------------------------------------------------------------------------

/// Most recent open heading at one logical document level.
#[derive(Clone)]
struct Heading {
    /// Parsed heading level.
    level: u8,
    /// Decoded heading identifier.
    id: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Parser {
    /// Prepares per-instance mappings before visiting the page HTML.
    pub fn new(
        tags: &Tags, source: &SourcePath, meta: &BTreeMap<String, Dynamic>,
    ) -> Result<Self> {
        let mut instances = Vec::new();
        let mut mappings = Vec::new();
        for instance in tags.instances.iter() {
            if !instance.filter.accepts(source)? {
                continue;
            }
            let tags = tag::normalize(
                meta.get(&instance.config.tags_name_property),
                &instance.config,
            )
            .with_context(|| {
                format!(
                    "error reading tags of page '{source}' for plugin '{}'",
                    instance.name
                )
            })?;
            mappings.push(Mapping { instance: instance.id, tags });
            instances.push(instance.clone());
        }
        Ok(Self {
            source: source.clone(),
            instances,
            mappings,
            listings: Vec::new(),
            headings: Vec::new(),
            pending: None,
            waiting: Vec::new(),
            error: None,
        })
    }

    /// Finishes cached page-local facts or reports a deferred visitor error.
    pub fn finish(self) -> Result<Facts> {
        if let Some(error) = self.error {
            Err(error)
        } else {
            Ok(Facts {
                mappings: self.mappings,
                listings: self.listings,
                search_cleanup: false,
            })
        }
    }

    /// Handles one tokenizer event without allowing malformed directives to
    /// prevent the remaining visitors from completing the shared pass.
    fn handle(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) -> Result<()> {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                if let Some(level) = heading_level(name) {
                    self.pending = Some(PendingHeading {
                        level,
                        id: None,
                        attribute_is_id: false,
                    });
                }
            }
            CallbackEvent::AttributeName { name } => {
                if let Some(pending) = &mut self.pending {
                    pending.attribute_is_id = *name == b"id";
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(pending) = &mut self.pending
                    && pending.attribute_is_id
                {
                    pending.id =
                        Some(String::from_utf8_lossy(value).into_owned());
                }
            }
            CallbackEvent::CloseStartTag { .. } => {
                if let Some(pending) = self.pending.take()
                    && let Some(id) = pending.id
                {
                    self.open_heading(pending.level, id);
                }
            }
            CallbackEvent::Comment { value } => {
                self.comment(value, span, editor)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Updates heading ownership and source-order anchors.
    fn open_heading(&mut self, level: u8, id: String) {
        for index in mem::take(&mut self.waiting) {
            let listing = &mut self.listings[index];
            if listing.host.is_none() || level > listing.host_level {
                listing.following = Some(id.clone());
            }
        }
        self.headings.retain(|heading| heading.level < level);
        self.headings.push(Heading { level, id });
    }

    /// Claims one directive for the first eligible matching instance.
    fn comment(
        &mut self, value: &[u8], span: Span<usize>, editor: &mut Editor<'_>,
    ) -> Result<()> {
        let value = String::from_utf8_lossy(value);
        let value = value.trim();
        for instance in &self.instances {
            let Some(arguments) =
                directive_arguments(value, &instance.config.listings_directive)
            else {
                continue;
            };
            let config =
                resolve(arguments, &instance.config).with_context(|| {
                    format!("error reading tags listing in '{}'", self.source)
                })?;
            if !instance.config.listings {
                editor.replace(span.start..span.end, Box::default());
                return Ok(());
            }

            let ordinal = u32::try_from(self.listings.len())
                .context("too many tag listings on one page")?;
            let mut nonce = 0_u32;
            let slot = loop {
                let candidate = format!(
                    "<!-- zensical:tags:{}:{ordinal}:{nonce} -->",
                    instance.id
                );
                if !editor.contains(&candidate) {
                    break candidate;
                }
                nonce = nonce
                    .checked_add(1)
                    .context("too many colliding tags listing markers")?;
            };
            let host = self
                .headings
                .iter()
                .rev()
                .find(|heading| heading.level < 6)
                .cloned();
            self.listings.push(Prepared {
                instance: instance.id,
                ordinal,
                slot: slot.clone(),
                host: host.as_ref().map(|heading| heading.id.clone()),
                host_level: host.as_ref().map_or(1, |heading| heading.level),
                following: None,
                config,
            });
            self.waiting.push(self.listings.len() - 1);
            editor.replace(span.start..span.end, slot);
            return Ok(());
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Visitor for Parser {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        if self.error.is_none()
            && let Err(error) = self.handle(event, span, editor)
        {
            self.error = Some(error);
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns a heading level for one HTML tag name.
fn heading_level(name: &[u8]) -> Option<u8> {
    match name {
        b"h1" => Some(1),
        b"h2" => Some(2),
        b"h3" => Some(3),
        b"h4" => Some(4),
        b"h5" => Some(5),
        b"h6" => Some(6),
        _ => None,
    }
}

/// Extracts a literal, case-insensitive directive's YAML tail.
fn directive_arguments<'a>(value: &'a str, directive: &str) -> Option<&'a str> {
    let prefix = value.get(..directive.len())?;
    if !prefix.eq_ignore_ascii_case(directive) {
        return None;
    }
    let tail = &value[directive.len()..];
    if tail.is_empty() || tail.starts_with(char::is_whitespace) {
        Some(tail.trim())
    } else {
        None
    }
}

/// Resolves empty, named, or inline YAML listing configuration.
fn resolve(
    arguments: &str, plugin: &TagsPluginConfig,
) -> Result<ListingConfig> {
    if arguments.is_empty() {
        return Ok(ListingConfig::default_for(plugin));
    }
    let documents = Yaml::load_from_str(arguments)?;
    if documents.len() != 1 {
        bail!("listing directive must contain exactly one YAML document")
    }
    let document = &documents[0];
    if let Some(name) = document.as_str() {
        let Some(value) = plugin.listings_map.get(name) else {
            let available = plugin
                .listings_map
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "couldn't find listing configuration: {name}. Available configurations: {available}"
            )
        };
        return Ok(ListingConfig::new(value, plugin));
    }
    let Some(mapping) = document.as_mapping() else {
        bail!("listing configuration must be a name or mapping")
    };
    let mut value = TagsListingConfig {
        scope: None,
        shadow: None,
        layout: None,
        toc: None,
        include: None,
        exclude: None,
    };
    for (key, item) in mapping {
        let Some(key) = key.as_str() else {
            bail!("listing configuration keys must be strings")
        };
        match key {
            "scope" => value.scope = Some(boolean(item, key)?),
            "shadow" => value.shadow = Some(boolean(item, key)?),
            "layout" => value.layout = Some(string(item, key)?),
            "toc" => value.toc = Some(boolean(item, key)?),
            "include" => value.include = Some(tags(item, key)?),
            "exclude" => value.exclude = Some(tags(item, key)?),
            _ => bail!("unknown listing configuration option: {key}"),
        }
    }
    Ok(ListingConfig::new(&value, plugin))
}

/// Requires a Boolean YAML value.
fn boolean(value: &Yaml<'_>, name: &str) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| anyhow!("listing option '{name}' must be a Boolean"))
}

/// Requires a string YAML value.
fn string(value: &Yaml<'_>, name: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("listing option '{name}' must be a string"))
}

/// Requires a Material-compatible iterable tag set.
fn tags(value: &Yaml<'_>, name: &str) -> Result<Vec<String>> {
    let Some(values) = value.as_sequence() else {
        bail!("listing option '{name}' must be an iterable tag set")
    };
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if let Some(value) = value.as_str() {
                Ok(value.to_owned())
            } else if let Some(value) = value.as_integer() {
                Ok(value.to_string())
            } else if let Some(value) = value.as_floating_point() {
                Ok(python_float(value))
            } else if let Some(value) = value.as_bool() {
                Ok(python_bool(value).into())
            } else {
                bail!("invalid tag at index {index} in listing option '{name}'")
            }
        })
        .collect()
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::directive_arguments;

    #[test]
    fn directive_names_are_literal_case_insensitive_tokens() {
        assert_eq!(
            directive_arguments(
                "MATERIAL/TAGS { toc: false }",
                "material/tags"
            ),
            Some("{ toc: false }")
        );
        assert_eq!(
            directive_arguments("material/tags-extra", "material/tags"),
            None
        );
    }
}
