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

//! Rendered HTML asset replacement and link annotation.

use anyhow::Result;
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use std::collections::BTreeMap;
use std::ops::Range;

use crate::compat::mkdocs::{
    html::{Editor, Visitor},
    url,
};
use crate::path::SitePath;

use super::{download, downloaded, Assets, Instance};

#[derive(Default)]
struct Parser {
    tags: Vec<Tag>,
    pending: Option<Tag>,
    attribute: String,
}

struct Tag {
    name: String,
    range: Range<usize>,
    attributes: BTreeMap<String, Option<String>>,
    self_closing: bool,
}

impl Tag {
    fn get(&self, name: &str) -> &str {
        self.attributes
            .get(name)
            .and_then(Option::as_deref)
            .unwrap_or("")
    }

    fn set(&mut self, name: &str, value: String) {
        self.attributes.insert(name.into(), Some(value));
    }

    fn target(&self) -> Option<(&str, &str)> {
        let attribute = match self.name.as_str() {
            "a" | "link" | "image" => "href",
            "script" | "img" => "src",
            _ => return None,
        };
        let value = self.get(attribute);
        let lower = value.to_ascii_lowercase();
        (lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("//"))
        .then_some((attribute, value))
    }

    fn render(&self) -> String {
        let mut html = format!("<{}", self.name);
        for (name, value) in &self.attributes {
            html.push(' ');
            html.push_str(name);
            if let Some(value) = value {
                html.push_str("=\"");
                html.push_str(&html_escape::encode_double_quoted_attribute(
                    value,
                ));
                html.push('"');
            }
        }
        html.push_str(if self.self_closing { " />" } else { ">" });
        html
    }
}

impl Visitor for Parser {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        _editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.pending = matches!(
                    *name,
                    b"a" | b"link" | b"script" | b"img" | b"image"
                )
                .then(|| Tag {
                    name: String::from_utf8_lossy(name).into_owned(),
                    range: span.start..span.end,
                    attributes: BTreeMap::new(),
                    self_closing: false,
                });
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute = String::from_utf8_lossy(name).into_owned();
                if let Some(tag) = &mut self.pending {
                    tag.attributes
                        .entry(self.attribute.clone())
                        .or_insert(None);
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(tag) = &mut self.pending {
                    tag.set(
                        &self.attribute,
                        String::from_utf8_lossy(value).into_owned(),
                    );
                }
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some(mut tag) = self.pending.take() {
                    tag.range.end = span.end;
                    tag.self_closing = *self_closing;
                    self.tags.push(tag);
                }
            }
            _ => {}
        }
    }
}

impl Instance {
    pub(super) fn html(
        &self, content: &str, destination: &SitePath, assets: &mut Assets,
    ) -> Result<String> {
        let mut parser = Parser::default();
        let mut editor = Editor::scan(content, &mut [&mut parser]);
        let mut urls = Vec::new();
        for tag in &parser.tags {
            let Some((_, value)) = tag.target() else {
                continue;
            };
            if matches!(tag.name.as_str(), "script" | "img" | "image")
                || tag.name == "link"
                    && matches!(
                        tag.get("rel"),
                        "icon" | "preload" | "stylesheet"
                    )
            {
                urls.push(value.to_owned());
            }
        }
        let graph = self.graph(urls, destination.as_str())?;
        for mut tag in parser.tags {
            let Some((attribute, value)) =
                tag.target().map(|(attribute, value)| {
                    (attribute.to_owned(), value.to_owned())
                })
            else {
                continue;
            };
            let mut changed = false;
            if tag.name == "a" && self.config.links {
                // Absolute same-host anchors are annotated by Material too;
                // only asset fetching applies its hostname exclusion rule.
                for (name, value) in &self.config.links_attr_map {
                    tag.set(name, value.clone());
                    changed = true;
                }
                if self.config.links_noopener && tag.get("target") == "_blank" {
                    let mut rel = tag
                        .get("rel")
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    if !rel.iter().any(|value| value == "noopener") {
                        rel.push("noopener".into());
                        tag.set("rel", rel.join(" "));
                        changed = true;
                    }
                }
            } else if tag.name == "link" && tag.get("rel") == "preconnect" {
                if self.accepts(&value, destination.as_str())? {
                    editor.replace(tag.range, "");
                    continue;
                }
            } else if (matches!(tag.name.as_str(), "script" | "img" | "image")
                || tag.name == "link"
                    && matches!(
                        tag.get("rel"),
                        "icon" | "preload" | "stylesheet"
                    ))
                && let Some(file) = downloaded(&graph, &value)
            {
                let path = download::public_path(&file.path);
                let mut replacement = if is_error_template(destination.as_str())
                {
                    let base = fluent_uri::UriRef::parse(self.site.as_str())
                        .ok()
                        .map(|uri| {
                            uri.path().as_str().trim_end_matches('/').to_owned()
                        })
                        .unwrap_or_default();
                    format!("{base}/{path}")
                } else {
                    url::relative(destination.as_str(), &path)
                };
                if let Some((_, fragment)) = value.split_once('#') {
                    replacement.push('#');
                    replacement.push_str(fragment);
                }
                tag.set(&attribute, replacement);
                changed = true;
            }
            if changed {
                editor.replace(tag.range.clone(), tag.render());
            }
        }
        self.emit(&graph, assets)?;
        Ok(editor.finish().unwrap_or_else(|| content.to_owned()))
    }
}

/// Root HTTP error templates are identified by MkDocs as three-digit names.
fn is_error_template(path: &str) -> bool {
    path.get(..3)
        .is_some_and(|status| status.bytes().all(|byte| byte.is_ascii_digit()))
        && matches!(path.get(3..), Some(".html" | ".htm"))
}
