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

//! Native mkdocs-llmstxt compatibility using the shared HTML pass.

use anyhow::{bail, Result};
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Stream, StreamSetExt, Value};

use crate::config::{plugins::LlmstxtPluginConfig, Config};
use crate::path::{SitePath, SourcePath};
use crate::structure::page::{Page, PageOrigin};
use crate::workflow::{output::Artifact, SourcesReady};

mod markdown;

pub use markdown::Parser;

/// Prepared configuration shared between page conversion and output generation.
#[derive(Clone, Debug)]
pub struct Llmstxt {
    settings: Arc<LlmstxtPluginConfig>,
    patterns: Arc<HashMap<String, std::result::Result<Regex, String>>>,
    base_url: String,
    header: String,
}

/// Selected page paired with Markdown from its shared HTML pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    /// Final page title, route and ownership.
    pub page: Page,
    /// GFM content aligned with the rendered page revision.
    pub markdown: Arc<str>,
}

/// Revision-settled selected pages, also used to seed an empty site.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Documents(Vec<Document>);

impl Value for Document {}
impl Value for Documents {}

impl Llmstxt {
    /// Prepares selectors and site metadata once per workflow.
    pub fn new(config: &Config) -> Self {
        let settings = &config.project.plugins.llmstxt.config;
        let patterns = settings
            .sections
            .iter()
            .flat_map(|section| {
                section.inputs.iter().filter_map(|input| {
                    input.expression.as_ref().map(|expression| {
                        (
                            input.pattern.clone(),
                            Regex::new(expression)
                                .map_err(|error| error.to_string()),
                        )
                    })
                })
            })
            .collect();
        let base_url = settings
            .base_url
            .as_ref()
            .or(config.project.site_url.as_ref())
            .map_or("", String::as_str);
        let base_url = format!("{}/", base_url.trim_end_matches('/'));
        let mut header = format!(
            "# {}\n\n",
            markdown::escape(
                &config.project.site_name.replace(['\r', '\n'], " ")
            )
        );
        if let Some(description) = &config.project.site_description {
            for line in description.lines() {
                let _ = writeln!(header, "> {}", markdown::escape(line));
            }
            header.push('\n');
        }
        if let Some(description) = &settings.markdown_description {
            header.push_str(description.trim());
            header.push_str("\n\n");
        }
        Self {
            settings: Arc::new(settings.clone()),
            patterns: Arc::new(patterns),
            base_url,
            header,
        }
    }

    /// Starts conversion only for pages selected by the configured sections.
    pub fn parser(
        &self, source: &SourcePath, url: &str,
    ) -> Result<Option<Parser>> {
        if self.settings.enabled {
            for section in &self.settings.sections {
                for input in &section.inputs {
                    if self.matches(&input.pattern, source.as_str())? {
                        return Ok(Some(Parser::new(
                            self.settings.autoclean,
                            markdown::absolute(&self.base_url, url),
                        )));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Index generation is enabled after initial source discovery settles.
    pub fn setup(
        &self, documents: &Stream<Id, Document>,
        sources_ready: &Stream<Id, SourcesReady>, strict: bool,
    ) -> Stream<Id, Artifact> {
        if !self.settings.enabled {
            return sources_ready.flat_map(|_: &SourcesReady| {
                Ok::<Vec<(Key<Id>, Artifact)>, anyhow::Error>(Vec::new())
            });
        }
        let markdown = documents.flat_map(|document: &Document| {
            Ok::<_, anyhow::Error>(vec![document.artifact()?])
        });
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
        let indexes = snapshot.product(sources_ready).flat_map(
            move |(documents, _): &(Documents, SourcesReady)| {
                pipeline.indexes(&documents.0, strict)
            },
        );
        (markdown, indexes).coalesce()
    }

    fn matches(&self, pattern: &str, source: &str) -> Result<bool> {
        match self.patterns.get(pattern) {
            Some(Ok(regex)) => Ok(regex.is_match(source)),
            Some(Err(error)) => {
                bail!("invalid llmstxt pattern {pattern:?}: {error}")
            }
            None => Ok(pattern == source),
        }
    }

    fn indexes(
        &self, documents: &[Document], strict: bool,
    ) -> Result<Vec<(Key<Id>, Artifact)>> {
        // Sorting source paths makes wildcard expansion independent of worker order.
        let pages = documents
            .iter()
            .map(|document| (document.page.source().as_str(), document))
            .collect::<BTreeMap<_, _>>();
        let mut index = self.header.clone();
        let mut full = self
            .settings
            .full_output
            .as_ref()
            .map(|_| self.header.clone());
        let mut warnings = false;
        for section in &self.settings.sections {
            let title =
                markdown::escape(&section.title.replace(['\r', '\n'], " "));
            let _ = write!(index, "## {title}\n\n");
            if let Some(full) = &mut full {
                let _ = write!(full, "# {title}\n\n");
            }
            let mut selected = Vec::<(&str, &str)>::new();
            let mut positions = HashMap::<&str, usize>::new();
            for input in &section.inputs {
                let matches = if self.patterns.contains_key(&input.pattern) {
                    pages
                        .keys()
                        .copied()
                        .filter_map(|source| {
                            match self.matches(&input.pattern, source) {
                                Ok(true) => Some(Ok(source)),
                                Ok(false) => None,
                                Err(error) => Some(Err(error)),
                            }
                        })
                        .collect::<Result<Vec<_>>>()?
                } else {
                    vec![input.pattern.as_str()]
                };
                for source in matches {
                    if let Some(position) = positions.get(source) {
                        selected[*position].1 = &input.description;
                    } else {
                        positions.insert(source, selected.len());
                        selected.push((source, &input.description));
                    }
                }
            }
            for (source, description) in selected {
                let Some(document) = pages.get(source) else {
                    eprintln!("WARNING -  llmstxt: Page URI '{source}' not found in the generated pages. Skipping.");
                    warnings = true;
                    continue;
                };
                let url = markdown::absolute(
                    &self.base_url,
                    &markdown_url(document.page.destination())?,
                );
                let title = markdown::escape(
                    &document.page.title.replace(['\r', '\n'], " "),
                );
                let _ = write!(
                    index,
                    "- [{title}]({})",
                    markdown::destination(&url)
                );
                if !description.is_empty() {
                    let _ = write!(
                        index,
                        ": {}",
                        description.replace(['\r', '\n'], " ")
                    );
                }
                index.push('\n');
                if let Some(full) = &mut full {
                    full.push_str(document.markdown.trim());
                    full.push_str("\n\n");
                }
            }
            index.push('\n');
        }
        if warnings && strict {
            bail!("Aborted because --strict flag is set");
        }

        let mut artifacts = vec![generated("llms.txt", index)?];
        if let (Some(name), Some(full)) = (&self.settings.full_output, full) {
            artifacts.push(generated(name, full)?);
        }
        Ok(artifacts)
    }
}

impl Document {
    /// Uses the page's owner so route changes retract the previous Markdown file.
    fn artifact(&self) -> Result<(Key<Id>, Artifact)> {
        Ok((
            key("page", self.page.source().as_str()),
            Artifact::page(
                self.page.origin().clone(),
                markdown_path(self.page.destination())?,
                self.markdown.as_bytes().to_vec(),
            ),
        ))
    }
}

/// The encoded site-relative Markdown URL is derived from the page destination.
pub(crate) fn markdown_url(destination: &SitePath) -> Result<String> {
    let path = markdown_path(destination)?;
    Ok(zensical_serve::http::Uri::from_parts(path.as_str(), "").to_string())
}

fn markdown_path(destination: &SitePath) -> Result<SitePath> {
    Ok(destination
        .with_file_name(&format!("{}.md", destination.file_stem()))?)
}

fn key(kind: &str, name: &str) -> Key<Id> {
    Key::from(
        Id::builder()
            .provider("llmstxt")
            .context(kind)
            .location(name)
            .build()
            .expect("valid output identity"),
    )
}

fn generated(name: &str, content: String) -> Result<(Key<Id>, Artifact)> {
    Ok((
        key("index", name),
        Artifact::page(
            PageOrigin::Generated {
                identity: format!("llmstxt:{name}"),
                provenance: None,
            },
            name.parse()?,
            markdown::document(&content).into_bytes(),
        ),
    ))
}
