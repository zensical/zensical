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

//! Tag normalization, hierarchy, sorting, and references.

use anyhow::{bail, Result};
use icu_casemap::CaseMapper;
use icu_locale_core::LanguageIdentifier;
use icu_normalizer::{ComposingNormalizer, DecomposingNormalizer};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::config::plugins::{python_scalar, TagsPluginConfig};
use crate::structure::dynamic::Dynamic;
use crate::structure::tag::{Tag as TemplateTag, TagNode as TemplateTagNode};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One normalized leaf tag and its cumulative hierarchy.
#[derive(Clone, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub struct Tag {
    /// Full leaf name.
    pub name: String,
    /// Root-to-leaf cumulative tag names.
    pub hierarchy: Vec<TagNode>,
}

// ----------------------------------------------------------------------------

/// One cumulative tag in a hierarchy.
#[derive(Clone, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub struct TagNode {
    /// Cumulative tag name.
    pub name: String,
    /// Parent tag, if this tag belongs to a hierarchy.
    pub parent: Option<Arc<TagNode>>,
    /// Stable public fragment.
    pub slug: String,
    /// Whether the tag is classified as a shadow tag.
    pub hidden: bool,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Tag {
    /// Returns whether this leaf or one of its parents has the given name.
    pub fn contains(&self, names: &BTreeSet<String>) -> bool {
        self.hierarchy.iter().any(|tag| names.contains(&tag.name))
    }

    /// Returns whether this leaf is hidden.
    pub fn hidden(&self) -> bool {
        self.hierarchy.last().is_some_and(|tag| tag.hidden)
    }

    /// Returns the template-visible leaf tag and its parent chain.
    pub fn template(&self) -> TemplateTagNode {
        template_node(
            self.hierarchy
                .last()
                .expect("configured tag always contains one hierarchy node"),
        )
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Normalizes one configured metadata property into deterministic leaf tags.
pub fn normalize(
    value: Option<&Dynamic>, config: &TagsPluginConfig,
) -> Result<Vec<Tag>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Dynamic::List(values) = value else {
        bail!("expected iterable tags, but received: {value}")
    };

    let allowed = config.tags_allowed.iter().cloned().collect::<BTreeSet<_>>();
    let mut names = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let name = python_scalar(value).ok_or_else(|| {
            anyhow::anyhow!(
                "expected a string, integer, float or Boolean tag at index {index}"
            )
        })?;
        if !allowed.is_empty() && !allowed.contains(&name) {
            bail!("tag not in allow list: {name}")
        }
        names.insert(name);
    }

    names
        .into_iter()
        .map(|name| configure(name, config))
        .collect()
}

/// Applies hierarchy, shadow, and slug configuration to one tag.
fn configure(name: String, config: &TagsPluginConfig) -> Result<Tag> {
    let components = if config.tags_hierarchy {
        if config.tags_hierarchy_separator.is_empty() {
            bail!("tags_hierarchy_separator must not be empty")
        }
        name.split(&config.tags_hierarchy_separator)
            .collect::<Vec<_>>()
    } else {
        vec![name.as_str()]
    };
    let shadows = config.shadow_tags.iter().cloned().collect::<BTreeSet<_>>();
    let mut hierarchy = Vec::with_capacity(components.len());
    let mut cumulative = String::new();
    let mut hidden = false;
    for (index, component) in components.into_iter().enumerate() {
        if index > 0 {
            cumulative.push_str(&config.tags_hierarchy_separator);
        }
        cumulative.push_str(component);
        hidden = hidden
            || shadows.contains(&cumulative)
            || (!config.shadow_tags_prefix.is_empty()
                && component.starts_with(&config.shadow_tags_prefix))
            || (!config.shadow_tags_suffix.is_empty()
                && component.ends_with(&config.shadow_tags_suffix));
        let parent = hierarchy.last().cloned().map(Arc::new);
        hierarchy.push(TagNode {
            name: cumulative.clone(),
            parent,
            slug: slug(&cumulative, config)?,
            hidden,
        });
    }
    Ok(Tag { name, hierarchy })
}

/// Projects one internal tag node into Material's fragment object model.
fn template_node(tag: &TagNode) -> TemplateTagNode {
    TemplateTagNode {
        name: tag.name.clone(),
        parent: tag.parent.as_deref().map(template_node).map(Box::new),
        hidden: tag.hidden,
    }
}

/// Produces the configured public fragment for one cumulative tag name.
pub fn slug(name: &str, config: &TagsPluginConfig) -> Result<String> {
    let parts = if config.tags_hierarchy {
        name.split(&config.tags_hierarchy_separator)
            .collect::<Vec<_>>()
    } else {
        vec![name]
    };
    let slug = parts
        .into_iter()
        .map(|part| {
            slug_part(
                part,
                &config.tags_slugify_separator,
                &config.tags_slugify,
            )
        })
        .collect::<Result<Vec<_>>>()?
        .join(&config.tags_hierarchy_separator);
    if !config.tags_slugify_format.contains("{slug}") {
        bail!("tags_slugify_format must contain '{{slug}}'")
    }
    Ok(config.tags_slugify_format.replace("{slug}", &slug))
}

/// Implements the supported Python Markdown and pymdownx slug strategies.
fn slug_part(value: &str, separator: &str, strategy: &str) -> Result<String> {
    match strategy {
        "pymdownx:lower" => Ok(slug_pymdownx(value, separator, false)),
        "pymdownx:fold" => Ok(slug_pymdownx(value, separator, true)),
        "markdown:slugify" => Ok(slug_markdown(value, separator)),
        _ => bail!("unsupported tags slug strategy: {strategy}"),
    }
}

/// Matches pymdownx's NFC, HTML stripping, case, and character policy.
fn slug_pymdownx(value: &str, separator: &str, fold: bool) -> String {
    let stripped = strip_html(value);
    let normalized = ComposingNormalizer::new_nfc().normalize(&stripped);
    let normalized = normalized.trim();
    let cased = if fold {
        CaseMapper::new().fold_string(normalized).into_owned()
    } else {
        CaseMapper::new()
            .lowercase_to_string(normalized, &LanguageIdentifier::UNKNOWN)
            .into_owned()
    };
    let mut output = String::with_capacity(cased.len());
    for character in cased.chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '-') {
            output.push(character);
        } else if character == ' ' {
            output.push_str(separator);
        }
    }
    output
}

/// Matches Python Markdown's ASCII NFKD slug function.
fn slug_markdown(value: &str, separator: &str) -> String {
    let normalized = DecomposingNormalizer::new_nfkd().normalize(value);
    let filtered = normalized
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, '_' | '-')
        })
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let mut output = String::with_capacity(filtered.len());
    let mut inside_separator = false;
    for character in filtered.trim().chars() {
        if character.is_whitespace() || separator.contains(character) {
            if !inside_separator {
                output.push_str(separator);
                inside_separator = true;
            }
        } else {
            inside_separator = false;
            output.push(character);
        }
    }
    output
}

/// Removes HTML tags using pymdownx's permissive non-nesting semantics.
fn strip_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        output.push_str(&rest[..start]);
        let candidate = &rest[start + 1..];
        if let Some(end) = candidate.find('>') {
            rest = &candidate[end + 1..];
        } else {
            output.push_str(&rest[start..]);
            return output;
        }
    }
    output.push_str(rest);
    output
}

/// Computes the Unicode default case-fold key used by Material sorting.
pub fn casefold(value: &str) -> String {
    CaseMapper::new().fold_string(value).into_owned()
}

/// Sorts page tag references according to the configured built-in strategy.
pub fn sort_references(
    references: &mut [TemplateTag], config: &TagsPluginConfig,
) -> Result<()> {
    match config.tags_sort_by.as_str() {
        "tag_name" => {
            references.sort_by(|left, right| left.name.cmp(&right.name));
        }
        "tag_name_casefold" => {
            references.sort_by_cached_key(|tag| casefold(&tag.name));
        }
        strategy => bail!("unsupported tags sort strategy: {strategy}"),
    }
    if config.tags_sort_reverse {
        references.reverse();
    }
    Ok(())
}

/// Groups references by their configured template variable.
pub fn variables(
    values: impl IntoIterator<Item = (String, Vec<TemplateTag>)>,
) -> BTreeMap<String, Vec<TemplateTag>> {
    let mut output = BTreeMap::new();
    for (name, references) in values {
        if references.is_empty() {
            continue;
        }
        output.entry(name).or_insert(references);
    }
    output
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{normalize, slug_part, variables, TemplateTag};
    use crate::config::plugins::TagsPluginConfig;
    use crate::structure::dynamic::Dynamic;

    fn config() -> TagsPluginConfig {
        TagsPluginConfig {
            tags_hierarchy: true,
            shadow_tags_prefix: "_".into(),
            ..TagsPluginConfig::default()
        }
    }

    #[test]
    fn normalizes_scalars_duplicates_hierarchy_and_shadow_state() {
        let tags = normalize(
            Some(&Dynamic::List(vec![
                Dynamic::String("Guide/_Internal".into()),
                Dynamic::String("Guide/_Internal".into()),
                Dynamic::Integer(7),
            ])),
            &config(),
        )
        .unwrap();

        assert_eq!(tags.len(), 2);
        let tag = tags
            .iter()
            .find(|tag| tag.name.starts_with("Guide"))
            .unwrap();
        assert_eq!(tag.hierarchy[0].name, "Guide");
        assert!(tag.hierarchy[0].parent.is_none());
        assert_eq!(
            tag.hierarchy[1]
                .parent
                .as_deref()
                .map(|parent| parent.name.as_str()),
            Some("Guide")
        );
        assert_eq!(tag.hierarchy[1].slug, "tag:guide/_internal");
        assert!(tag.hidden());
    }

    #[test]
    fn preserves_leading_empty_hierarchy_components() {
        let tags = normalize(
            Some(&Dynamic::List(vec![Dynamic::String("//Child".into())])),
            &config(),
        )
        .unwrap();
        let hierarchy = &tags[0].hierarchy;

        assert_eq!(
            hierarchy
                .iter()
                .map(|tag| tag.name.as_str())
                .collect::<Vec<_>>(),
            ["", "/", "//Child"]
        );
        assert_eq!(hierarchy[2].slug, "tag://child");
        assert_eq!(
            hierarchy[2]
                .parent
                .as_deref()
                .map(|parent| parent.name.as_str()),
            Some("/")
        );
    }

    #[test]
    fn matches_supported_python_slug_strategies() {
        assert_eq!(
            slug_part("<b>A  Straße</b>", "-", "pymdownx:lower").unwrap(),
            "a--straße"
        );
        assert_eq!(
            slug_part("Straße ςΣ", "-", "pymdownx:fold").unwrap(),
            "strasse-σσ"
        );
        assert_eq!(
            slug_part("Café\tand  more", "-", "markdown:slugify").unwrap(),
            "cafe-and-more"
        );
        assert_eq!(slug_part("a<b", "-", "pymdownx:lower").unwrap(), "ab");
        assert_eq!(
            slug_part("-Edge-", "-", "markdown:slugify").unwrap(),
            "-edge-"
        );
        assert_eq!(slug_part("a---b", "-", "markdown:slugify").unwrap(), "a-b");
        assert_eq!(slug_part("ΣΣ", "-", "pymdownx:lower").unwrap(), "σς");
        assert_eq!(slug_part("!!!", "-", "pymdownx:lower").unwrap(), "");
    }

    #[test]
    fn empty_references_do_not_claim_a_shared_template_variable() {
        let populated = TemplateTag {
            name: "Visible".into(),
            parent: None,
            url: None,
            hidden: false,
            links: Vec::new(),
        };
        let variables = variables([
            ("tags".into(), Vec::new()),
            ("tags".into(), vec![populated.clone()]),
        ]);

        assert_eq!(variables["tags"], [populated]);
    }
}
