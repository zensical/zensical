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

//! Page metadata and content used by both feed formats.

use anyhow::{Context, Result};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};
use pyo3::prelude::*;
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::compat::mkdocs::plugin::rss::Entry;
use crate::config::plugins::RssPluginConfig;
use crate::path::SourceRoot;
use crate::structure::dynamic::Dynamic;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Image metadata shared by RSS enclosures and JSON Feed items.
#[derive(Clone, Debug)]
pub struct Image {
    /// Public image URL.
    pub url: String,
    /// Guessed media type for RSS enclosures.
    pub mime: Option<String>,
    /// Local byte length for RSS enclosures.
    pub size: Option<u64>,
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Matches the unescaped characters in Python's urlencode/quote_plus.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'-')
    .remove(b'.')
    .remove(b'~');

/// Appends ordered query parameters to a page's canonical URL.
pub fn link(entry: &Entry, settings: &RssPluginConfig) -> Option<String> {
    let mut url = entry.page.canonical_url.clone()?;
    if !settings.url_parameters.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        let parameters = settings
            .url_parameters
            .iter()
            .map(|(key, value)| {
                let encode = |value: &str| {
                    percent_encoding::utf8_percent_encode(value, QUERY)
                        .to_string()
                        .replace("%20", "+")
                };
                format!("{}={}", encode(key), encode(value))
            })
            .collect::<Vec<_>>()
            .join("&");
        url.push_str(&parameters);
    }
    Some(url)
}

/// Returns the page's comments URL when configured.
pub fn comments(entry: &Entry, settings: &RssPluginConfig) -> Option<String> {
    Some(format!(
        "{}{}",
        entry.page.canonical_url.as_ref()?,
        settings.comments_path.as_ref()?
    ))
}

/// Selects or renders the page description for a feed item.
pub fn description(
    entry: &Entry, settings: &RssPluginConfig,
) -> Result<String> {
    if settings.abstract_chars_count == -1 {
        // Native page rendering includes default heading permalink controls.
        // Material's RSS page-content hook runs before those controls appear.
        static HEADERLINK: OnceLock<Regex> = OnceLock::new();
        let pattern = HEADERLINK.get_or_init(|| {
            Regex::new(r#"<a\b[^>]*\bclass="headerlink"[^>]*>[^<]*</a>"#)
                .expect("fixed heading permalink pattern is valid")
        });
        return Ok(pattern.replace_all(&entry.page.content, "").into_owned());
    }
    if let Some(Dynamic::Map(rss)) = entry.page.meta.get("rss")
        && let Some(Dynamic::String(value)) = rss.get("feed_description")
        && !value.is_empty()
    {
        return Ok(value.clone());
    }
    if let Some(Dynamic::String(value)) = entry.page.meta.get("description")
        && !value.is_empty()
    {
        return Ok(value.clone());
    }
    if settings.abstract_chars_count == 0 {
        return Ok(String::new());
    }
    let raw = if settings.abstract_delimiter.is_empty() {
        entry.body.as_ref()
    } else {
        entry
            .body
            .split_once(&settings.abstract_delimiter)
            .map_or(entry.body.as_ref(), |(before, _)| before)
    };
    let raw = if raw.len() == entry.body.len()
        && settings.abstract_chars_count >= 0
    {
        let limit = usize::try_from(settings.abstract_chars_count).unwrap_or(0);
        if raw.chars().count() > limit {
            format!(
                "{}...",
                raw.chars()
                    .take(limit.saturating_sub(3))
                    .collect::<String>()
            )
        } else {
            raw.to_owned()
        }
    } else {
        raw.to_owned()
    };
    if raw.is_empty() {
        return Ok(String::new());
    }
    Python::attach(|py| {
        py.import("markdown")?
            .call_method1("markdown", (raw,))?
            .extract::<String>()
    })
    .context("rendering rss description")
}

/// Resolves explicit and Material blog authors in plugin precedence order.
pub fn authors(entry: &Entry, settings: &RssPluginConfig) -> Vec<String> {
    let meta = &entry.page.meta;
    if let Some(value) = meta.get("author") {
        return strings(value);
    }
    if settings.use_material_blog
        && let Some(Dynamic::List(values)) = entry.page.property("authors")
        && !values.is_empty()
    {
        return values
            .iter()
            .filter_map(|value| match value {
                Dynamic::Map(author) => match author.get("name") {
                    Some(Dynamic::String(name)) => {
                        let email = match author.get("email") {
                            Some(Dynamic::String(email)) => Some(email),
                            _ => None,
                        };
                        Some(email.map_or_else(
                            || name.clone(),
                            |email| format!("{email} ({name})"),
                        ))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
    }
    meta.get("authors").map(strings).unwrap_or_default()
}

/// Extracts string values from a scalar or list metadata field.
fn strings(value: &Dynamic) -> Vec<String> {
    match value {
        Dynamic::String(value) => vec![value.clone()],
        Dynamic::List(values) => values
            .iter()
            .filter_map(|value| match value {
                Dynamic::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Collects and sorts categories from the configured metadata fields.
pub fn categories(entry: &Entry, settings: &RssPluginConfig) -> Vec<String> {
    let mut values = settings
        .categories
        .iter()
        .filter_map(|key| entry.page.meta.get(key))
        .flat_map(strings)
        .collect::<Vec<_>>();
    values.sort();
    values
}

/// Resolves a page image or illustration.
pub fn image(
    entry: &Entry, docs: &SourceRoot, base: Option<&str>,
) -> Option<Image> {
    let value = entry
        .page
        .meta
        .get("image")
        .or_else(|| entry.page.meta.get("illustration"));
    let value = value?;
    let Dynamic::String(value) = value else {
        return None;
    };
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return Some(Image {
            url: value.into(),
            mime: mime(value),
            size: None,
        });
    }
    let source = entry.page.source().as_str();
    let parent = source.rsplit_once('/').map_or("", |(parent, _)| parent);
    let mut path = PathBuf::from(docs.as_path());
    path.push(parent);
    path.push(value);
    let canonical = fs::canonicalize(&path).ok()?;
    let relative = canonical.strip_prefix(docs.as_path()).ok()?;
    let size = fs::metadata(&canonical).ok()?.len();
    let relative = relative.to_str()?.replace('\\', "/");
    Some(Image {
        url: format!("{}{}", base.unwrap_or(""), relative),
        mime: mime(value),
        size: Some(size),
    })
}

/// Guesses the MIME type from a supported image extension.
fn mime(value: &str) -> Option<String> {
    let extension = value.rsplit_once('.')?.1.to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => return None,
    };
    Some(mime.into())
}
