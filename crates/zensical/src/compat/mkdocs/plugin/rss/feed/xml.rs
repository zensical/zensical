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

//! RSS 2.0 serialization.

use anyhow::Result;
use std::fmt::Write;

use crate::compat::mkdocs::plugin::rss::{Entry, Stamp};
use crate::config::plugins::RssPluginConfig;
use crate::config::Project;
use crate::path::SourceRoot;

use super::item::{
    authors, categories, comments, description, image, link, Image,
};
use super::{base_url, feed_url};

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Serializes one ordered page list as RSS 2.0.
// The channel and its ordered items are written into one output buffer.
#[allow(clippy::too_many_lines)]
pub fn render(
    project: &Project, docs: &SourceRoot, settings: &RssPluginConfig,
    name: &str, entries: &[Entry], kind: &str, now: &Stamp,
) -> Result<String> {
    let sep = if settings.pretty_print { "\n" } else { "" };
    let mut xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{sep}");
    if !settings.stylesheet.is_empty() {
        let stylesheet = if settings.stylesheet == "auto" {
            "rss.xsl"
        } else {
            &settings.stylesheet
        };
        write!(
            xml,
            "<?xml-stylesheet type=\"text/xsl\" href=\"{}\"?>{sep}",
            escape(stylesheet)
        )?;
    }
    write!(xml, "<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\"><channel>{sep}")?;
    element(
        &mut xml,
        "title",
        settings.feed_title.as_deref().unwrap_or(&project.site_name),
        sep,
    )?;
    if let Some(description) = settings
        .feed_description
        .as_ref()
        .or(project.site_description.as_ref())
    {
        element(&mut xml, "description", description, sep)?;
    }
    if let Some(base) = base_url(project) {
        element(&mut xml, "link", &base, sep)?;
    }
    if let Some(url) = feed_url(project, name) {
        write!(xml, "<atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\" />{sep}", escape(&url))?;
    }
    if let Some(author) = &project.site_author {
        element(&mut xml, "managingEditor", author, sep)?;
    }
    if let Some(repo) = &project.repo_url {
        element(&mut xml, "docs", repo, sep)?;
    }
    element(&mut xml, "language", &project.theme.language, sep)?;
    element(&mut xml, "pubDate", &now.rss, sep)?;
    element(&mut xml, "lastBuildDate", &now.rss, sep)?;
    element(&mut xml, "ttl", &settings.feed_ttl.to_string(), sep)?;
    element(&mut xml, "generator", "Zensical RSS", sep)?;
    if let Some(url) = &settings.image {
        write!(xml, "<image>{sep}")?;
        element(&mut xml, "url", url, sep)?;
        element(
            &mut xml,
            "title",
            settings.feed_title.as_deref().unwrap_or(&project.site_name),
            sep,
        )?;
        if let Some(base) = base_url(project) {
            element(&mut xml, "link", &base, sep)?;
        }
        write!(xml, "</image>{sep}")?;
    }
    for entry in entries {
        write!(xml, "<item>{sep}")?;
        element(&mut xml, "title", &entry.page.title, sep)?;
        for author in authors(entry, settings) {
            element(&mut xml, "author", &author, sep)?;
        }
        for category in categories(entry, settings) {
            element(&mut xml, "category", &category, sep)?;
        }
        element(&mut xml, "description", &description(entry, settings)?, sep)?;
        if let Some(link) = link(entry, settings) {
            element(&mut xml, "link", &link, sep)?;
            if let Some(source) = feed_url(project, name) {
                write!(
                    xml,
                    "<source url=\"{}\">{}</source>{sep}",
                    escape(&source),
                    escape(
                        settings
                            .feed_title
                            .as_deref()
                            .unwrap_or(&project.site_name)
                    )
                )?;
            }
        }
        element(
            &mut xml,
            "pubDate",
            if kind == "created" {
                &entry.created.as_ref().unwrap_or(now).rss
            } else {
                &entry.updated.as_ref().unwrap_or(now).rss
            },
            sep,
        )?;
        if let Some(comments) = comments(entry, settings) {
            element(&mut xml, "comments", &comments, sep)?;
        }
        if let Some(guid) = &entry.page.canonical_url {
            write!(
                xml,
                "<guid isPermaLink=\"true\">{}</guid>{sep}",
                escape(guid)
            )?;
        }
        if let Some(Image {
            url,
            mime: Some(mime),
            size: Some(size),
        }) = image(entry, docs, base_url(project).as_deref())
        {
            write!(
                xml,
                "<enclosure url=\"{}\" type=\"{}\" length=\"{size}\" />{sep}",
                escape(&url),
                escape(&mime)
            )?;
        }
        write!(xml, "</item>{sep}")?;
    }
    write!(xml, "</channel></rss>{sep}")?;
    Ok(xml)
}

/// Appends one escaped XML element.
fn element(
    output: &mut String, name: &str, value: &str, sep: &str,
) -> Result<()> {
    write!(output, "<{name}>{}</{name}>{sep}", escape(value))?;
    Ok(())
}

/// Escapes text and attribute values for the feed's XML output.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
