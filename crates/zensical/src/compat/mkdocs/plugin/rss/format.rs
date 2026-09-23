// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

//! RSS 2.0 and JSON Feed 1.1 serialization.

use anyhow::{Context, Result};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};
use pyo3::prelude::*;
use serde_json::{json, Value as Json};
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

use zrx::id::Id;
use zrx::stream::Key;

use crate::config::plugins::RssPluginConfig;
use crate::config::Project;
use crate::path::{SitePath, SourceRoot};
use crate::structure::dynamic::Dynamic;
use crate::structure::page::PageOrigin;
use crate::workflow::output::Artifact;

use super::date::Stamp;
use super::{candidate_key, Entry};

#[derive(Clone, Debug)]
struct Image {
    url: String,
    mime: Option<String>,
    size: Option<u64>,
}

/// Matches the unescaped characters in Python's urlencode/quote_plus.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'-')
    .remove(b'.')
    .remove(b'~');

pub(super) fn artifacts(
    project: &Project, docs: &SourceRoot, settings: &RssPluginConfig,
    instance: usize, emit_stylesheet: bool, created: &[Entry],
    updated: &[Entry], now: &Stamp,
) -> Result<Vec<(Key<Id>, Artifact)>> {
    let mut output = Vec::new();
    let variants = [
        (
            "created",
            created,
            &settings.feeds_filenames.rss_created,
            &settings.feeds_filenames.json_created,
        ),
        (
            "updated",
            updated,
            &settings.feeds_filenames.rss_updated,
            &settings.feeds_filenames.json_updated,
        ),
    ];
    for (kind, entries, rss_name, json_name) in variants {
        if settings.rss_feed_enabled {
            let bytes =
                rss_xml(project, docs, settings, rss_name, entries, kind, now)?;
            output.push(artifact(instance, rss_name, bytes.into_bytes())?);
        }
        if settings.json_feed_enabled {
            let value = json_feed(project, docs, settings, json_name, entries)?;
            let bytes = if settings.pretty_print {
                serde_json::to_vec_pretty(&value)?
            } else {
                serde_json::to_vec(&value)?
            };
            output.push(artifact(instance, json_name, bytes)?);
        }
    }
    if emit_stylesheet {
        output.push(artifact(
            instance,
            "rss.xsl",
            include_bytes!("default.xsl").to_vec(),
        )?);
    }
    Ok(output)
}

fn artifact(
    instance: usize, name: &str, contents: Vec<u8>,
) -> Result<(Key<Id>, Artifact)> {
    let destination: SitePath = name.parse()?;
    Ok((
        candidate_key("rss-output", instance, name),
        Artifact::page(
            PageOrigin::Generated {
                identity: format!("rss:{instance}:{name}"),
                provenance: None,
            },
            destination,
            contents,
        ),
    ))
}

fn json_feed(
    project: &Project, docs: &SourceRoot, settings: &RssPluginConfig,
    name: &str, entries: &[Entry],
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
                "date_published": entry.created.json,
                "date_modified": entry.updated.json,
                "authors": authors(entry, settings).into_iter()
                    .map(|name| json!({"name": name})).collect::<Vec<_>>(),
                "tags": categories(entry, settings),
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

fn rss_xml(
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
                &entry.created.rss
            } else {
                &entry.updated.rss
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

fn element(
    output: &mut String, name: &str, value: &str, sep: &str,
) -> Result<()> {
    write!(output, "<{name}>{}</{name}>{sep}", escape(value))?;
    Ok(())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn base_url(project: &Project) -> Option<String> {
    project
        .site_url
        .as_ref()
        .filter(|url| !url.is_empty())
        .map(|url| format!("{}/", url.trim_end_matches('/')))
}

fn feed_url(project: &Project, name: &str) -> Option<String> {
    base_url(project).map(|base| format!("{base}{name}"))
}

fn link(entry: &Entry, settings: &RssPluginConfig) -> Option<String> {
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

fn comments(entry: &Entry, settings: &RssPluginConfig) -> Option<String> {
    Some(format!(
        "{}{}",
        entry.page.canonical_url.as_ref()?,
        settings.comments_path.as_ref()?
    ))
}

fn description(entry: &Entry, settings: &RssPluginConfig) -> Result<String> {
    if settings.abstract_chars_count == -1 {
        return Ok(entry.page.content.clone());
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
    let raw = if !settings.abstract_delimiter.is_empty() {
        entry
            .body
            .split_once(&settings.abstract_delimiter)
            .map(|(before, _)| before)
            .unwrap_or(&entry.body)
    } else {
        &entry.body
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

fn authors(entry: &Entry, settings: &RssPluginConfig) -> Vec<String> {
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

fn categories(entry: &Entry, settings: &RssPluginConfig) -> Vec<String> {
    let mut values = settings
        .categories
        .iter()
        .filter_map(|key| entry.page.meta.get(key))
        .flat_map(strings)
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn image(
    entry: &Entry, docs: &SourceRoot, base: Option<&str>,
) -> Option<Image> {
    let value = entry
        .page
        .meta
        .get("image")
        .or_else(|| entry.page.meta.get("illustration"));
    let Some(value) = value else {
        return entry.social_image.as_ref().map(|(url, size)| Image {
            url: url.clone(),
            mime: Some("image/png".into()),
            size: Some(*size),
        });
    };
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
