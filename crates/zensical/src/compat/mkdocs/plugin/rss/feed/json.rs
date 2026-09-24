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

//! JSON Feed 1.1 serialization.

use anyhow::Result;
use serde_json::{json, Value as Json};

use crate::compat::mkdocs::plugin::rss::Entry;
use crate::config::plugins::RssPluginConfig;
use crate::config::Project;
use crate::path::SourceRoot;

use super::item::{authors, categories, description, image, link};
use super::{base_url, feed_url};

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Serializes one ordered page list as JSON Feed 1.1.
pub fn render(
    project: &Project, docs: &SourceRoot, settings: &RssPluginConfig,
    name: &str, entries: &[Entry], now: &super::Stamp,
) -> Result<Json> {
    let base = base_url(project);
    let items = entries
        .iter()
        .map(|entry| {
            let image = image(entry, docs, base.as_deref());
            Ok(json!({
                "id": entry.page.canonical_url,
                "url": link(entry, settings),
                "title": entry.page.title,
                "content_html": description(entry, settings)?,
                "image": image.map(|image| image.url),
                "date_published": entry.created.as_ref().unwrap_or(now).json,
                "date_modified": entry.updated.as_ref().unwrap_or(now).json,
                "authors": authors(entry, settings).into_iter()
                    .map(|name| json!({"name": name})).collect::<Vec<_>>(),
                "tags": (!settings.categories.is_empty())
                    .then(|| categories(entry, settings)),
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "version": "https://jsonfeed.org/version/1.1",
        "title": settings.feed_title.as_deref().unwrap_or(&project.site_name),
        "home_page_url": base,
        "feed_url": feed_url(project, name),
        "description": settings.feed_description.as_ref().or(project.site_description.as_ref()),
        "icon": settings.image,
        "authors": project.site_author.iter().map(|name| json!({"name": name})).collect::<Vec<_>>(),
        "language": project.theme.language,
        "items": items,
    }))
}
