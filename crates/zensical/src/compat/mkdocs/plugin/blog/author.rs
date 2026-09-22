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

//! Native author-catalog parsing and post author resolution.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use zrx::stream::Value;

use crate::compat::mkdocs::plugin::meta;
use crate::config::plugins::BlogPluginConfig;
use crate::path::SourcePath;
use crate::structure::dynamic::Dynamic;

use super::BlogId;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One validated author definition.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    /// Stable author identifier used by post metadata.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Profile description.
    pub description: String,
    /// Avatar URL or documentation-relative path.
    pub avatar: String,
    /// Optional explicit profile slug.
    pub slug: Option<String>,
    /// Optional explicit author URL.
    pub url: Option<String>,
}

/// Authors loaded for one configured blog instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    /// Owning blog instance.
    pub blog: BlogId,
    /// Authors keyed by stable metadata identifier.
    pub authors: BTreeMap<String, Author>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Catalog {
    /// Parses and validates Material's authors-file mapping.
    pub fn parse(
        blog: BlogId, path: SourcePath, source: &str,
    ) -> anyhow::Result<Self> {
        let mut values = meta::parse_yaml(path.clone(), source)
            .with_context(|| format!("error reading authors file '{path}'"))?;
        let unknown = values
            .keys()
            .filter(|name| name.as_str() != "authors")
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            bail!(
                "authors file '{path}' has unknown option(s): {}",
                unknown.join(", ")
            )
        }
        let authors = match values.remove("authors") {
            None | Some(Dynamic::Null) => BTreeMap::new(),
            Some(Dynamic::Map(authors)) => authors,
            Some(_) => bail!("'authors' in '{path}' must be a mapping"),
        };
        let authors = authors
            .into_iter()
            .map(|(id, value)| {
                parse_author(&path, id.clone(), value)
                    .map(|author| (id, author))
            })
            .collect::<anyhow::Result<_>>()?;
        Ok(Self { blog, authors })
    }
}

impl Author {
    /// Formats this author's configured profile path below the blog root.
    pub fn profile_path(&self, settings: &BlogPluginConfig) -> String {
        settings
            .authors_profiles_url_format
            .replace("{slug}", self.slug.as_ref().unwrap_or(&self.id))
            .replace("{name}", &self.name)
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Catalog {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Resolves the configured authors-file path for one blog instance.
pub fn source(settings: &BlogPluginConfig) -> anyhow::Result<SourcePath> {
    let path = settings
        .authors_file
        .replace("{blog}", settings.blog_dir.trim_matches('/'));
    path.strip_prefix("./")
        .unwrap_or(&path)
        .parse()
        .context("invalid authors_file path")
}

fn parse_author(
    path: &SourcePath, id: String, value: Dynamic,
) -> anyhow::Result<Author> {
    let Dynamic::Map(mut values) = value else {
        bail!("author '{id}' in '{path}' must be a mapping")
    };
    let unknown = values
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "name" | "description" | "avatar" | "slug" | "url"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        bail!(
            "author '{id}' in '{path}' has unknown option(s): {}",
            unknown.join(", ")
        )
    }
    let name = required_string(&mut values, &id, path, "name")?;
    let description = required_string(&mut values, &id, path, "description")?;
    let avatar = required_string(&mut values, &id, path, "avatar")?;
    let slug = optional_string(&mut values, &id, path, "slug")?;
    let url = optional_string(&mut values, &id, path, "url")?;
    Ok(Author {
        id,
        name,
        description,
        avatar,
        slug,
        url,
    })
}

fn required_string(
    values: &mut BTreeMap<String, Dynamic>, id: &str, path: &SourcePath,
    name: &str,
) -> anyhow::Result<String> {
    let Some(value) = values.remove(name) else {
        bail!("author '{id}' in '{path}' is missing '{name}'")
    };
    match value {
        Dynamic::String(value) => Ok(value),
        _ => {
            bail!("author '{id}' option '{name}' in '{path}' must be a string")
        }
    }
}

fn optional_string(
    values: &mut BTreeMap<String, Dynamic>, id: &str, path: &SourcePath,
    name: &str,
) -> anyhow::Result<Option<String>> {
    match values.remove(name) {
        None | Some(Dynamic::Null) => Ok(None),
        Some(Dynamic::String(value)) => Ok(Some(value)),
        Some(_) => {
            bail!("author '{id}' option '{name}' in '{path}' must be a string")
        }
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{Author, BlogId, Catalog};

    #[test]
    fn parses_material_author_mappings() {
        let catalog = Catalog::parse(
            BlogId(0),
            "blog/.authors.yml".parse().unwrap(),
            "authors:\n  jane:\n    name: Jane\n    description: Writer\n    avatar: jane.png\n",
        )
        .unwrap();
        assert_eq!(
            catalog.authors["jane"],
            Author {
                id: "jane".into(),
                name: "Jane".into(),
                description: "Writer".into(),
                avatar: "jane.png".into(),
                slug: None,
                url: None,
            }
        );
    }

    #[test]
    fn normalizes_the_default_standalone_authors_path() {
        let settings = crate::config::plugins::BlogPluginConfig {
            blog_dir: ".".into(),
            ..crate::config::plugins::BlogPluginConfig::default()
        };
        assert_eq!(super::source(&settings).unwrap().as_str(), ".authors.yml");
    }
}
