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

//! Blog post classification, metadata, and route derivation.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use zrx::stream::Value;

use crate::config::plugins::{BlogPluginConfig, ExcerptPolicy};
use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::document::DocumentHeader;
use crate::structure::dynamic::Dynamic;
use crate::structure::page::{PageDescriptor, PageOrigin, PageRoute};
use crate::structure::slug;

use super::links::{self, LinkItem};
use super::{Author, BlogDate, BlogId, PostId};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Validated blog interpretation of one Markdown document.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostDescriptor {
    /// Stable source-based identity.
    pub id: PostId,
    /// Pre-render page descriptor with the post's final route.
    pub page: PageDescriptor,
    /// Structured post dates. `created` is always present.
    pub dates: BTreeMap<String, BlogDate>,
    /// Unique author identifiers in declaration order.
    pub author_ids: Vec<String>,
    /// Resolved author objects in declaration order.
    pub authors: Vec<Author>,
    /// Unique category names in declaration order.
    pub categories: Vec<String>,
    /// Whether the post is pinned in views.
    pub pin: bool,
    /// Whether metadata explicitly marks this post as a draft.
    pub draft: Option<bool>,
    /// Explicit slug, if supplied.
    pub slug: Option<String>,
    /// Optional explicit read-time override.
    pub readtime: Option<usize>,
    /// Parsed navigation-shaped related links.
    pub links: Option<Vec<LinkItem>>,
}

/// Validated metadata used to construct template-facing post properties.
struct PostProperties<'a> {
    /// Structured post dates keyed by semantic role.
    dates: &'a BTreeMap<String, BlogDate>,
    /// Unique category names in declaration order.
    categories: &'a [String],
    /// Whether the post sorts before ordinary posts.
    pin: bool,
    /// Explicit draft metadata, when present.
    draft: Option<bool>,
    /// Explicit read-time override, when present.
    readtime: Option<usize>,
    /// Original structured related-link metadata, when present.
    links: Option<&'a Dynamic>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl PostDescriptor {
    /// Classifies and validates a document below one blog's post directory.
    pub fn from_document(
        config: &Config, blog: BlogId, settings: &BlogPluginConfig,
        mut document: DocumentHeader,
    ) -> anyhow::Result<Option<Self>> {
        let post_dir = post_dir(settings)?;
        if document.source.parent().as_ref() != Some(&post_dir)
            && !document.source.is_descendant_of(&post_dir)
        {
            return Ok(None);
        }

        let dates = dates(&document)?;
        let created = dates
            .get("created")
            .copied()
            .expect("created date is validated");
        let author_ids = string_list(&document, "authors")?;
        let categories = string_list(&document, "categories")?;
        if !settings.categories_allowed.is_empty() {
            for category in &categories {
                if !settings.categories_allowed.contains(category) {
                    bail!(
                        "post '{}' uses category '{}' outside categories_allowed",
                        document.source,
                        category
                    )
                }
            }
        }
        let pin = optional_bool(&document, "pin")?.unwrap_or(false);
        let draft = optional_bool(&document, "draft")?;
        let slug = optional_string(&document, "slug")?;
        let readtime = optional_usize(&document, "readtime")?;
        let links = links::parse(document.meta.get("links"))?;
        if settings.post_excerpt == ExcerptPolicy::Required
            && !document.body.contains(&settings.post_excerpt_separator)
        {
            bail!(
                "post '{}' requires the excerpt separator '{}'",
                document.source,
                settings.post_excerpt_separator
            )
        }

        document
            .meta
            .entry("template".into())
            .or_insert_with(|| Dynamic::String("blog-post.html".into()));
        hide_navigation(&mut document)?;

        let route_source = route_source(
            settings,
            &document,
            created,
            &categories,
            slug.as_deref(),
        )?;
        let destination = PageRoute::destination(
            &route_source,
            config.project.use_directory_urls,
        )?;
        let route = PageRoute::from_destination(
            config,
            document.source.clone(),
            destination,
        );
        let id = PostId {
            blog,
            source: document.source.clone(),
        };
        let properties = properties(
            config,
            settings,
            PostProperties {
                dates: &dates,
                categories: &categories,
                pin,
                draft,
                readtime,
                links: document.meta.get("links"),
            },
        )?;
        let page = PageDescriptor {
            origin: PageOrigin::Source(document.source.clone()),
            document,
            route,
            properties,
            variables: BTreeMap::from([(
                "_blog_date_format".into(),
                Dynamic::String(settings.post_date_format.clone()),
            )]),
        };
        Ok(Some(Self {
            id,
            page,
            dates,
            author_ids,
            authors: Vec::new(),
            categories,
            pin,
            draft,
            slug,
            readtime,
            links,
        }))
    }

    /// Returns the mandatory creation date.
    pub fn created(&self) -> BlogDate {
        self.dates["created"]
    }

    /// Returns whether this post is hidden by draft policy at `now`.
    pub fn is_excluded(
        &self, settings: &BlogPluginConfig, serve: bool, now: i64,
    ) -> bool {
        let include_drafts =
            settings.draft || (serve && settings.draft_on_serve);
        if include_drafts {
            return false;
        }
        self.draft.unwrap_or_else(|| {
            settings.draft_if_future_date
                && self.created().timestamp_micros() > now
        })
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for PostDescriptor {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn properties(
    config: &Config, settings: &BlogPluginConfig, input: PostProperties<'_>,
) -> anyhow::Result<BTreeMap<String, Dynamic>> {
    let mut post = BTreeMap::new();
    post.insert(
        "date".into(),
        Dynamic::Map(
            input
                .dates
                .iter()
                .map(|(name, date)| {
                    (name.clone(), Dynamic::String(date.template_value()))
                })
                .collect(),
        ),
    );
    post.insert("pin".into(), Dynamic::Bool(input.pin));
    if let Some(draft) = input.draft {
        post.insert("draft".into(), Dynamic::Bool(draft));
    }
    if let Some(readtime) = input.readtime {
        post.insert(
            "readtime".into(),
            Dynamic::Integer(
                i64::try_from(readtime)
                    .context("readtime exceeds native integer range")?,
            ),
        );
    }
    if let Some(links) = input.links {
        post.insert("links".into(), links.clone());
    }
    let root = settings.blog_dir.trim_matches('/');
    let source = if matches!(root, "" | ".") {
        "index.md".parse::<SourcePath>()?
    } else {
        format!("{root}/index.md").parse()?
    };
    let parent = PageRoute::from_source(config, source)?;
    let categories = if settings.categories {
        input
            .categories
            .iter()
            .map(|name| {
                let source = super::category_source(settings, name)?;
                let route = PageRoute::from_source(config, source)?;
                Ok(Dynamic::Map(BTreeMap::from([
                    ("title".into(), Dynamic::String(name.clone())),
                    ("url".into(), Dynamic::String(route.url)),
                ])))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    Ok(BTreeMap::from([
        ("config".into(), Dynamic::Map(post)),
        ("authors".into(), Dynamic::List(Vec::new())),
        ("categories".into(), Dynamic::List(categories)),
        (
            "parent".into(),
            Dynamic::Map(BTreeMap::from([(
                "url".into(),
                Dynamic::String(parent.url),
            )])),
        ),
    ]))
}

fn hide_navigation(document: &mut DocumentHeader) -> anyhow::Result<()> {
    let hide = document
        .meta
        .entry("hide".into())
        .or_insert_with(|| Dynamic::List(Vec::new()));
    let Dynamic::List(values) = hide else {
        bail!("post '{}': hide must be a list", document.source)
    };
    if !values
        .iter()
        .any(|value| value == &Dynamic::String("navigation".into()))
    {
        values.push(Dynamic::String("navigation".into()));
    }
    Ok(())
}

pub(super) fn post_dir(
    settings: &BlogPluginConfig,
) -> anyhow::Result<SourcePath> {
    let path = settings.post_dir.replace("{blog}", blog_root(settings));
    path.trim_matches('/')
        .strip_prefix("./")
        .unwrap_or(path.trim_matches('/'))
        .parse()
        .context("invalid blog post_dir")
}

fn route_source(
    settings: &BlogPluginConfig, document: &DocumentHeader, created: BlogDate,
    categories: &[String], explicit_slug: Option<&str>,
) -> anyhow::Result<SourcePath> {
    let slug = explicit_slug.map_or_else(
        || slug::unicode(&document.title, &settings.post_slugify_separator),
        ToOwned::to_owned,
    );
    let categories = categories
        .iter()
        .take(settings.post_url_max_categories)
        .map(|category| {
            slug::unicode(category, &settings.categories_slugify_separator)
        })
        .collect::<Vec<_>>()
        .join("/");
    let date = created.format_url(&settings.post_url_date_format)?;
    let path = settings
        .post_url_format
        .replace("{categories}", &categories)
        .replace("{date}", &date)
        .replace("{file}", document.source.file_stem())
        .replace("{slug}", &slug);
    let path = path.trim_matches('/');
    if path.is_empty() {
        bail!("post '{}' produces an empty URL", document.source)
    }
    let root = blog_root(settings);
    let source = if root.is_empty() {
        format!("{path}.md")
    } else {
        format!("{root}/{path}.md")
    };
    source.parse().with_context(|| {
        format!("invalid route for post '{}'", document.source)
    })
}

fn blog_root(settings: &BlogPluginConfig) -> &str {
    match settings.blog_dir.trim_matches('/') {
        "." => "",
        root => root,
    }
}

fn dates(
    document: &DocumentHeader,
) -> anyhow::Result<BTreeMap<String, BlogDate>> {
    let value = document.meta.get("date").ok_or_else(|| {
        anyhow::anyhow!("post '{}' requires date metadata", document.source)
    })?;
    let values = match value {
        Dynamic::Map(values) => values.clone(),
        value => BTreeMap::from([("created".into(), value.clone())]),
    };
    let mut dates = BTreeMap::new();
    for (name, value) in values {
        let Dynamic::String(value) = value else {
            bail!(
                "post '{}': date.{name} must be a date or datetime",
                document.source
            )
        };
        dates.insert(
            name.clone(),
            BlogDate::parse(&value).with_context(|| {
                format!(
                    "post '{}': invalid date.{name} value '{value}'",
                    document.source
                )
            })?,
        );
    }
    if !dates.contains_key("created") {
        bail!("post '{}': date.created is required", document.source)
    }
    Ok(dates)
}

fn string_list(
    document: &DocumentHeader, name: &str,
) -> anyhow::Result<Vec<String>> {
    let Some(value) = document.meta.get(name) else {
        return Ok(Vec::new());
    };
    let Dynamic::List(values) = value else {
        bail!("post '{}': {name} must be a list", document.source)
    };
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let Dynamic::String(value) = value else {
            bail!("post '{}': {name} entries must be strings", document.source)
        };
        if seen.insert(value.clone()) {
            result.push(value.clone());
        }
    }
    Ok(result)
}

fn optional_bool(
    document: &DocumentHeader, name: &str,
) -> anyhow::Result<Option<bool>> {
    match document.meta.get(name) {
        None | Some(Dynamic::Null) => Ok(None),
        Some(Dynamic::Bool(value)) => Ok(Some(*value)),
        Some(_) => {
            bail!("post '{}': {name} must be a Boolean", document.source)
        }
    }
}

fn optional_string(
    document: &DocumentHeader, name: &str,
) -> anyhow::Result<Option<String>> {
    match document.meta.get(name) {
        None | Some(Dynamic::Null) => Ok(None),
        Some(Dynamic::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("post '{}': {name} must be a string", document.source),
    }
}

fn optional_usize(
    document: &DocumentHeader, name: &str,
) -> anyhow::Result<Option<usize>> {
    match document.meta.get(name) {
        None | Some(Dynamic::Null) => Ok(None),
        Some(Dynamic::Integer(value)) => usize::try_from(*value)
            .map(Some)
            .map_err(anyhow::Error::from),
        Some(_) => bail!(
            "post '{}': {name} must be a non-negative integer",
            document.source
        ),
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::structure::dynamic::Dynamic;

    use super::{
        dates, route_source, BlogDate, BlogPluginConfig, DocumentHeader,
    };

    fn document(meta: BTreeMap<String, Dynamic>) -> DocumentHeader {
        DocumentHeader::new(
            "blog/posts/hello.md".parse().unwrap(),
            "# Héllo, World!".into(),
            meta,
        )
    }

    #[test]
    fn parses_scalar_and_structured_dates() {
        let scalar = document(BTreeMap::from([(
            "date".into(),
            Dynamic::String("2026-09-03".into()),
        )]));
        assert_eq!(
            dates(&scalar).unwrap()["created"],
            BlogDate::parse("2026-09-03").unwrap()
        );

        let structured = document(BTreeMap::from([(
            "date".into(),
            Dynamic::Map(BTreeMap::from([
                ("created".into(), Dynamic::String("2026-09-03".into())),
                ("updated".into(), Dynamic::String("2026-09-04".into())),
            ])),
        )]));
        assert_eq!(dates(&structured).unwrap().len(), 2);
    }

    #[test]
    fn derives_native_unicode_routes() {
        let document = document(BTreeMap::default());
        let route = route_source(
            &BlogPluginConfig::default(),
            &document,
            BlogDate::parse("2026-09-03").unwrap(),
            &[],
            None,
        )
        .unwrap();
        assert_eq!(route.as_str(), "blog/2026/09/03/héllo-world.md");
    }
}
