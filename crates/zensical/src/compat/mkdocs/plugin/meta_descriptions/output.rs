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

//! CSV output from final rendered page descriptions.

use anyhow::{bail, Result};
use fluent_uri::{Uri, UriRef};
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Stream, StreamSetExt, Value};

use crate::compat::mkdocs::html::{self, Editor, Visitor};
use crate::structure::page::{Page, PageOrigin};
use crate::workflow::{output::Artifact, SourcesReady};

use super::{has_description, MetaDescriptions};

/// Description read from the final HTML, paired with the published page URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    source: String,
    url: String,
    description: Option<String>,
    explicit: bool,
    generated: bool,
}

impl Value for Document {}

/// Current exported pages, including an empty snapshot after page removals.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Documents(Vec<Document>);

impl Value for Documents {}

/// First description meta tag before the body.
#[derive(Default)]
struct Parser {
    tag: Vec<u8>,
    attribute: Vec<u8>,
    description_tag: bool,
    content: Option<String>,
    description: Option<String>,
    done: bool,
}

impl MetaDescriptions {
    /// CSV fields are collected from final HTML, including custom templates.
    pub fn document(
        &self, page: &Page, content: &str, generated: bool,
    ) -> Option<Document> {
        if !self.settings.enabled {
            return None;
        }
        let mut parser = Parser::default();
        if self.settings.export_csv {
            html::scan(content, &mut [&mut parser]);
        }
        Some(Document {
            source: page.source().to_string(),
            url: page.url.clone(),
            description: parser.description,
            explicit: !generated
                && has_description(page.meta.get("description")),
            generated,
        })
    }

    /// An owned CSV artifact is produced from revision-settled rendered pages.
    pub fn setup(
        &self, documents: &Stream<Id, Document>,
        sources_ready: &Stream<Id, SourcesReady>, strict: bool,
    ) -> Stream<Id, Artifact> {
        if !self.settings.enabled {
            return sources_ready.flat_map(|_: &SourcesReady| {
                Ok::<Vec<(Key<Id>, Artifact)>, anyhow::Error>(Vec::new())
            });
        }
        let seed = sources_ready.map(|_: &SourcesReady| Documents(Vec::new()));
        let pages = documents
            .map(|document: &Document| Documents(vec![document.clone()]));
        let snapshot = (seed, pages).coalesce().reduce(
            |pages: &dyn Collection<Key<Id>, Documents>| {
                Some(Documents(
                    pages
                        .values()
                        .flat_map(|pages| pages.0.iter().cloned())
                        .collect(),
                ))
            },
        );
        let pipeline = self.clone();
        snapshot.product(sources_ready).flat_map(
            move |(pages, _): &(Documents, SourcesReady)| {
                pipeline.export(&pages.0, strict)
            },
        )
    }

    fn export(
        &self, documents: &[Document], strict: bool,
    ) -> Result<Vec<(Key<Id>, Artifact)>> {
        if !self.settings.quiet {
            let explicit =
                documents.iter().filter(|page| page.explicit).count();
            let generated =
                documents.iter().filter(|page| page.generated).count();
            let count = explicit + generated;
            let total = documents.len();
            eprintln!("INFO    -  meta-descriptions: Added meta descriptions to {count} of {total} pages, {generated} using the first paragraph");
        }
        if !self.settings.export_csv || documents.is_empty() {
            return Ok(Vec::new());
        }
        // Source order is used so CSV output is independent of worker order.
        let mut pages = documents.iter().collect::<Vec<_>>();
        pages.sort_by(|left, right| left.source.cmp(&right.source));
        let mut csv = String::from("Page,Meta description\r\n");
        let mut missing = 0;
        for page in pages {
            let url = Uri::parse(self.site_url.as_str())
                .ok()
                .and_then(|base| {
                    UriRef::parse(page.url.as_str())
                        .ok()?
                        .resolve_against(&base)
                        .ok()
                })
                .map_or_else(|| page.url.clone(), |uri| uri.to_string());
            csv.push_str(&field(&url));
            csv.push(',');
            if let Some(description) = &page.description {
                csv.push_str(&field(description));
            } else {
                missing += 1;
            }
            csv.push_str("\r\n");
        }
        if missing > 0 {
            let warning = format!(
                "Couldn't find meta descriptions for {missing} HTML pages"
            );
            eprintln!("WARNING -  meta-descriptions: {warning}");
            if strict {
                bail!("meta-descriptions: {warning}");
            }
        }
        if !self.settings.quiet {
            eprintln!("INFO    -  meta-descriptions: Exported meta descriptions to meta-descriptions.csv");
        }
        let name = "meta-descriptions.csv";
        let key = Id::builder()
            .provider("meta-descriptions")
            .context("export")
            .location(name)
            .build()
            .expect("valid export identity");
        Ok(vec![(
            Key::from(key),
            Artifact::page(
                PageOrigin::Generated {
                    identity: format!("meta-descriptions:{name}"),
                    provenance: None,
                },
                name.parse()?,
                csv.into_bytes(),
            ),
        )])
    }
}

impl Visitor for Parser {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, _: Span<usize>,
        _: &mut Editor<'_>,
    ) {
        if self.done {
            return;
        }
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.done = *name == b"body";
                self.tag = name.to_vec();
                self.attribute.clear();
                self.description_tag = false;
                self.content = None;
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute = name.to_vec();
            }
            CallbackEvent::AttributeValue { value } if self.tag == b"meta" => {
                match self.attribute.as_slice() {
                    b"name" => self.description_tag = *value == b"description",
                    b"content" => {
                        self.content =
                            Some(String::from_utf8_lossy(value).into_owned());
                    }
                    _ => {}
                }
            }
            CallbackEvent::CloseStartTag { .. }
                if self.tag == b"meta" && self.description_tag =>
            {
                self.description =
                    Some(self.content.take().unwrap_or_default());
                self.done = true;
            }
            _ => {}
        }
    }
}

fn field(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.into()
    }
}

#[cfg(test)]
mod tests {
    use super::{field, Parser};
    use crate::compat::mkdocs::html;

    #[test]
    fn final_meta_tag_is_decoded_and_body_tags_are_excluded() {
        let mut parser = Parser::default();

        html::scan(
            "<head><META content='A &amp; B' name=description></head>\
            <body><meta name=description content=ignored>",
            &mut [&mut parser],
        );

        assert_eq!(parser.description.as_deref(), Some("A & B"));

        let mut parser = Parser::default();
        html::scan(
            "<body><meta name=description content=ignored>",
            &mut [&mut parser],
        );

        assert_eq!(parser.description, None);
    }

    #[test]
    fn csv_fields_preserve_quotes_and_newlines() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a,\"b\"\nc"), "\"a,\"\"b\"\"\nc\"");
    }
}
