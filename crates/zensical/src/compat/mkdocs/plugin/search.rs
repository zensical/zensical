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

//! MkDocs-compatible search plugin.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::sync::Arc;

use zrx::id::Id;
use zrx::scheduler::Value;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal, Stream};

use crate::config::plugins::SearchPluginConfig;
use crate::config::Config;
use crate::path::{OutputRoot, SitePath, SourcePath};
use crate::structure::dynamic::Dynamic;
use crate::structure::nav::{source_sort_key, Navigation};
use crate::structure::page::Page;

mod item;
mod parser;

use item::{SearchItem, SearchSection};
use parser::Parser;

/// Default separator used by the browser search worker.
const DEFAULT_SEPARATOR: &str = r#"[\s\-_,:!=\[\]()\\"`/]+|\.(?!\d)"#;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MkDocs-compatible search pipeline.
#[derive(Clone, Debug)]
pub struct Search {
    /// Normalized tokenizer configuration.
    config: SearchPluginConfig,
    /// Theme language used by the tokenizer.
    language: String,
    /// Whether the offline JavaScript index is emitted.
    offline: bool,
    /// Site output directory.
    output: OutputRoot,
}

// ----------------------------------------------------------------------------

/// Inputs required to derive and write search artifacts.
pub struct Dependencies<'a> {
    /// Page-local search documents.
    pub documents: &'a Stream<Id, Document>,
    /// Revision-complete site navigation.
    pub navigation: &'a Signal<Id, Navigation>,
}

// ----------------------------------------------------------------------------

/// Search configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SearchConfig {
    /// Languages for tokenizer.
    lang: Vec<String>,
    /// Separator for tokenizer.
    separator: String,
}

/// Complete search artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SearchIndex {
    /// Search configuration.
    config: SearchConfig,
    /// Search items.
    items: Vec<SearchItem>,
}

/// Search facts extracted while rendering one Markdown page.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Facts {
    /// Page-local search sections.
    sections: Vec<SearchSection>,
}

/// Compact page document retained by the search branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    /// Documentation-relative source used for deterministic ordering.
    source: SourcePath,
    /// Page target URL.
    url: String,
    /// Page title.
    title: String,
    /// Page tag names.
    tags: Vec<String>,
    /// Page-local facts extracted before page construction.
    facts: Arc<Facts>,
}

/// Complete current search-document relation at the artifact boundary.
#[derive(Clone, Debug)]
struct Documents(Arc<Vec<Document>>);

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Search {
    /// Resolves the private settings owned by this pipeline instance.
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.project.plugins.search.config.clone(),
            language: config.project.theme.language.clone(),
            offline: config.project.plugins.offline.config.enabled,
            output: config.output_root().clone(),
        }
    }

    /// Returns whether search extraction participates in page processing.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Installs revision-complete search artifact generation.
    pub fn setup(&self, dependencies: Dependencies<'_>) {
        let documents = collect_documents(dependencies.documents);
        let search = self.clone();
        let _ = documents.product(dependencies.navigation).map(
            move |documents: &Documents, navigation: &Navigation| {
                let documents = if search.config.enabled {
                    documents.0.as_ref().clone()
                } else {
                    Vec::new()
                };
                let index = SearchIndex::new(
                    documents,
                    navigation,
                    search.config.clone(),
                    &search.language,
                )?;
                search.write(&index)
            },
        );
    }

    /// Writes search artifacts without retaining serialized copies.
    fn write(&self, index: &SearchIndex) -> anyhow::Result<()> {
        let path = self.output.join(
            &"search.json".parse::<SitePath>().expect("static site path"),
        );
        fs::create_dir_all(path.parent().expect("invariant"))?;
        let mut writer = BufWriter::new(fs::File::create(path)?);
        serde_json::to_writer(&mut writer, index)?;
        writer.flush()?;

        if self.offline {
            let path = self.output.join(
                &"search.js".parse::<SitePath>().expect("static site path"),
            );
            fs::create_dir_all(path.parent().expect("invariant"))?;
            let mut writer = BufWriter::new(fs::File::create(path)?);
            writer.write_all(b"var __index = ")?;
            serde_json::to_writer(&mut writer, index)?;
            writer.write_all(b";")?;
            writer.flush()?;
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------

impl SearchConfig {
    /// Creates search configuration for the configured theme language.
    fn new(config: SearchPluginConfig, language: &str) -> Self {
        Self {
            lang: vec![language.to_string()],
            separator: config.separator,
        }
    }
}

// ----------------------------------------------------------------------------

impl Document {
    /// Attaches page properties to previously extracted search facts.
    pub fn new(page: &Page, facts: Arc<Facts>) -> Self {
        Self {
            source: page.source().clone(),
            url: page.url.clone(),
            title: page.title.clone(),
            tags: page.tags().into_iter().map(|tag| tag.name).collect(),
            facts,
        }
    }
}

// ----------------------------------------------------------------------------

impl Facts {
    /// Returns whether this page contributes anything to the search index.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

// ----------------------------------------------------------------------------

impl SearchIndex {
    /// Creates a search index from compact page facts.
    #[allow(clippy::assigning_clones)]
    fn new(
        documents: Vec<Document>, nav: &Navigation, config: SearchPluginConfig,
        language: &str,
    ) -> anyhow::Result<Self> {
        let count = documents
            .iter()
            .map(|document| document.facts.sections.len())
            .sum();
        let limit = Self::field_limit(count)?;
        let mut items: Vec<SearchItem> = Vec::with_capacity(count);

        // Provider order is not stable, so establish MkDocs-compatible source
        // order before emitting the site-wide index.
        let mut documents = documents;
        documents.sort_by_key(|document| source_sort_key(&document.source));

        // Attach site-wide navigation facts only while assembling the final
        // artifact, keeping them out of each page-local stream value.
        for document in documents {
            let iter = nav.ancestors_for_url(&document.url).into_iter().rev();
            let mut path = iter
                .filter_map(|item| {
                    item.display_title().map(ToString::to_string)
                })
                .collect::<Vec<_>>();

            // Add page title to path if not already present - this might be
            // the true in case of index pages
            if path.last() != Some(&document.title) {
                path.push(document.title.clone());
            }

            // Each heading section becomes an independently addressable search
            // item while sharing the page path and tags.
            for section in &document.facts.sections {
                let location = match &section.location {
                    Some(id) => format!("{}#{}", document.url, id),
                    _ => document.url.clone(),
                };
                let title = if section.title.is_empty() {
                    document.title.clone()
                } else {
                    section.title.clone()
                };
                let item = SearchItem {
                    location: Some(location),
                    level: section.level,
                    title,
                    text: section.text.clone(),
                    path: path.clone(),
                    tags: document.tags.clone(),
                };
                Self::validate_item(&item, count, limit, &config.separator)?;
                items.push(item);
            }
        }

        Ok(Self {
            config: SearchConfig::new(config, language),
            items,
        })
    }

    /// Computes a conservative field byte limit for the final item count.
    fn field_limit(count: usize) -> anyhow::Result<usize> {
        // Disco indexes title, text and path, reserving two field bits. Its
        // current tokenizer emits at most one token per source code unit, so
        // UTF-8 byte lengths conservatively bound UTF-16 token positions.
        let document_bits =
            (usize::BITS - count.saturating_sub(1).leading_zeros()).max(1);
        30_u32
            .checked_sub(document_bits)
            .and_then(|bits| 1_usize.checked_shl(bits))
            .ok_or_else(|| {
                anyhow::anyhow!("Search index has too many items: {count}")
            })
    }

    /// Checks each item as it is constructed, before adding it to the index.
    fn validate_item(
        item: &SearchItem, count: usize, limit: usize, separator: &str,
    ) -> anyhow::Result<()> {
        for (field, input, length) in [
            ("title", Some(item.title.as_str()), item.title.len()),
            ("text", Some(item.text.as_str()), item.text.len()),
            ("path", None, item.path.iter().map(String::len).sum()),
        ] {
            if length <= limit {
                continue;
            }

            let location = item
                .location
                .as_deref()
                .filter(|path| !path.is_empty())
                .unwrap_or("/");

            // The byte length is a safe upper bound for token positions. For
            // the default separator, count actual positions only when that
            // bound is exceeded. Custom JavaScript regexes keep the bound.
            if separator == DEFAULT_SEPARATOR {
                let positions = input.map_or_else(
                    || item.path.iter().map(|part| count_tokens(part)).sum(),
                    count_tokens,
                );
                anyhow::ensure!(
                    positions <= limit,
                    "Search field '{field}' at '{location}' exceeds the search \
                     index limit ({positions} token positions; maximum {limit} \
                     for {count} items). Shorten this field, split the page \
                     into smaller sections, or exclude it from search."
                );
            } else {
                anyhow::bail!(
                    "Search field '{field}' at '{location}' exceeds the search index \
                 limit ({length} bytes; maximum {limit} bytes for {count} \
                 items). Shorten this field, split the page into smaller \
                 sections, or exclude it from search."
                );
            }
        }
        Ok(())
    }
}

/// Counts Disco token positions for the default separator without allocating.
fn count_tokens(input: &str) -> usize {
    // Disco emits text between angle brackets as separate blocks. This also
    // preserves its behavior for incomplete or nested markup.
    let mut count = 0;
    let mut start = 0;
    for (index, character) in input.char_indices() {
        match character {
            '<' => {
                if index > start {
                    count += count_tokens_in_block(&input[start..index]);
                    start = index;
                }
            }
            '>' => start = index + 1,
            _ => {}
        }
    }
    if start < input.len() {
        count += count_tokens_in_block(&input[start..]);
    }
    count
}

/// Counts runs between separators in a Disco text block.
fn count_tokens_in_block(input: &str) -> usize {
    let mut count = 0;
    let mut token = false;
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        let separator = matches!(
            character,
            '\t'..='\r'
                | ' '
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
                | '-'
                | '_'
                | ','
                | ':'
                | '!'
                | '='
                | '['
                | ']'
                | '('
                | ')'
                | '\\'
                | '"'
                | '`'
                | '/'
        ) || character == '.'
            && !matches!(characters.peek(), Some('0'..='9'));
        if separator {
            token = false;
        } else if !token {
            count += 1;
            token = true;
        }
    }
    count
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Document {}
impl Value for Documents {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Collects the current document relation at its artifact boundary.
fn collect_documents(
    documents: &Stream<Id, Document>,
) -> Signal<Id, Documents> {
    documents.reduce(|documents: &dyn Collection<Key<Id>, Document>| {
        Some(Documents(Arc::new(documents.values().cloned().collect())))
    })
}

/// Creates the page-local search visitor.
pub fn parser(meta: &BTreeMap<String, Dynamic>) -> Parser {
    if is_search_excluded(meta) {
        Parser::discarding()
    } else {
        Parser::default()
    }
}

/// Converts a completed visitor into cached page-local facts.
pub fn finish(parser: Parser) -> Arc<Facts> {
    Arc::new(Facts { sections: parser.finish() })
}

/// Returns whether a page is excluded from search through its metadata.
fn is_search_excluded(meta: &BTreeMap<String, Dynamic>) -> bool {
    let Some(Dynamic::Map(search)) = meta.get("search") else {
        return false;
    };
    matches!(search.get("exclude"), Some(Dynamic::Bool(true)))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use zrx::id::Id;
    use zrx::stream::function::Collection;
    use zrx::stream::{Change, Key, Run, Workflow};

    use crate::path::SourcePath;
    use crate::structure::dynamic::Dynamic;

    use super::{
        collect_documents, count_tokens, is_search_excluded, Document,
        Documents, Facts, SearchIndex, SearchItem, DEFAULT_SEPARATOR,
    };

    fn search_item() -> SearchItem {
        SearchItem {
            location: Some(String::from("guide/#section")),
            level: 1,
            title: String::new(),
            text: String::new(),
            path: Vec::new(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn search_fields_respect_position_capacity_at_item_count_boundaries() {
        assert_eq!(SearchIndex::field_limit(0).unwrap(), 1 << 29);
        assert_eq!(SearchIndex::field_limit(1).unwrap(), 1 << 29);
        assert!(SearchIndex::field_limit(usize::MAX).is_err());

        for (count, limit, field) in [
            (16_384, 65_536, "text"),
            (32_768, 32_768, "title"),
            (32_769, 16_384, "text"),
            (65_536, 16_384, "text"),
            (65_537, 8_192, "text"),
        ] {
            assert_eq!(SearchIndex::field_limit(count).unwrap(), limit);
            let mut item = search_item();
            if field == "title" {
                item.title = "a ".repeat(limit);
            } else {
                item.text = "a ".repeat(limit);
            }
            SearchIndex::validate_item(&item, count, limit, DEFAULT_SEPARATOR)
                .unwrap();

            if field == "title" {
                item.title.push('a');
            } else {
                item.text.push('a');
            }
            let error = SearchIndex::validate_item(
                &item,
                count,
                limit,
                DEFAULT_SEPARATOR,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(&format!("Search field '{field}'")));
            assert!(error.contains("guide/#section"));
            assert!(error.contains(&format!("maximum {limit} for")));
        }
    }

    #[test]
    fn search_path_limit_combines_components_for_custom_separator() {
        let mut item = search_item();
        item.path = vec!["é".repeat(4_096), "a".repeat(8_192)];
        let separator = r"[\s-]+";
        SearchIndex::validate_item(&item, 32_769, 16_384, separator).unwrap();

        item.path.push(String::from("a"));
        let error =
            SearchIndex::validate_item(&item, 32_769, 16_384, separator)
                .unwrap_err()
                .to_string();
        assert!(error.contains("Search field 'path'"));
        assert!(error.contains("16385 bytes; maximum 16384 bytes"));
    }

    #[test]
    fn default_search_path_counts_tokens_across_components() {
        let mut item = search_item();
        item.path = vec!["word ".repeat(16_384), "word ".repeat(16_384)];
        SearchIndex::validate_item(&item, 22_449, 32_768, DEFAULT_SEPARATOR)
            .unwrap();

        item.path[1].push_str("word");
        let error = SearchIndex::validate_item(
            &item,
            22_449,
            32_768,
            DEFAULT_SEPARATOR,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Search field 'path'"));
        assert!(error.contains("32769 token positions; maximum 32768"));
    }

    #[test]
    fn long_default_search_field_uses_token_positions() {
        let mut item = search_item();
        item.text =
            format!("<pre><code>{}</code></pre>", "word ".repeat(7_500));
        assert!(item.text.len() > 32_768);
        assert_eq!(count_tokens(&item.text), 7_500);
        SearchIndex::validate_item(&item, 22_449, 32_768, DEFAULT_SEPARATOR)
            .unwrap();

        let error =
            SearchIndex::validate_item(&item, 22_449, 32_768, r"[\s-]+")
                .unwrap_err()
                .to_string();
        assert!(error.contains("bytes; maximum 32768 bytes"));
    }

    #[test]
    fn default_search_tokenizer_matches_markup_and_separator_edges() {
        assert_eq!(count_tokens("one<span>two</span>three"), 3);
        assert_eq!(count_tokens("3.14 foo.bar .5"), 4);
        assert_eq!(count_tokens("one\u{a0}two\u{feff}three"), 3);
        assert_eq!(count_tokens("<unfinished"), 1);
        assert_eq!(count_tokens("one>two"), 1);
        assert_eq!(count_tokens("a<b<c>d"), 3);
        assert_eq!(count_tokens("a..2 b.3"), 3);
        assert_eq!(count_tokens("x\u{2028}y\u{85}z"), 2);
    }

    fn document(source: &str, title: &str) -> Document {
        Document {
            source: source.parse::<SourcePath>().unwrap(),
            url: source.replace(".md", ".html"),
            title: title.to_owned(),
            tags: Vec::new(),
            facts: Arc::new(Facts::default()),
        }
    }

    fn key(location: &str) -> Key<Id> {
        Key::from(
            Id::builder()
                .provider("test")
                .context("search")
                .location(location)
                .build()
                .unwrap(),
        )
    }

    fn snapshot(run: &mut Run<Id>) -> (Vec<String>, usize, usize) {
        let changes = run
            .output::<((Documents, usize), usize)>()
            .unwrap()
            .collect::<Vec<_>>();
        let [Change::Insert(_, ((documents, count), title_bytes))] =
            changes.as_slice()
        else {
            panic!("expected one coherent search snapshot, got {changes:?}");
        };
        let mut titles = documents
            .0
            .iter()
            .map(|document| document.title.clone())
            .collect::<Vec<_>>();
        titles.sort();
        (titles, *count, *title_bytes)
    }

    fn rendered(run: &mut Run<Id>) -> Vec<Option<(String, usize, usize)>> {
        let mut changes = run
            .output::<((Document, usize), usize)>()
            .unwrap()
            .map(|change| match change {
                Change::Insert(_, ((document, count), title_bytes)) => {
                    Some((document.title, count, title_bytes))
                }
                Change::Remove(_) => None,
            })
            .collect::<Vec<_>>();
        changes.sort();
        changes
    }

    #[test]
    fn search_exclusion_is_read_from_page_metadata() {
        let mut search = BTreeMap::new();
        search.insert(String::from("exclude"), Dynamic::Bool(true));
        let mut meta = BTreeMap::new();
        meta.insert(String::from("search"), Dynamic::Map(search));

        assert!(is_search_excluded(&meta));

        meta.clear();
        assert!(!is_search_excluded(&meta));
    }

    #[test]
    fn page_relations_converge_with_sibling_site_facts() {
        let workflow = Workflow::<Id>::build(|workflow| {
            let source = workflow.input::<Document>();
            let documents = collect_documents(&source);
            let count = source.reduce(
                |documents: &dyn Collection<Key<Id>, Document>| {
                    Some(documents.len())
                },
            );
            let title_bytes = source.reduce(
                |documents: &dyn Collection<Key<Id>, Document>| {
                    Some(
                        documents
                            .values()
                            .map(|document| document.title.len())
                            .sum::<usize>(),
                    )
                },
            );
            workflow.output(&documents.product(&count).product(&title_bytes));
            workflow.output(&source.product(&count).product(&title_bytes));
        });
        let mut runner = workflow.runner().unwrap();
        let input = runner.input::<Document>().unwrap();

        let mut revision = input.begin().unwrap();
        revision
            .insert(key("a.md"), document("a.md", "Alpha"))
            .unwrap();
        revision
            .insert(key("b.md"), document("b.md", "Beta"))
            .unwrap();
        let input = revision.seal().unwrap();
        let mut run = runner.settle().unwrap();
        assert_eq!(
            snapshot(&mut run),
            (vec![String::from("Alpha"), String::from("Beta")], 2, 9)
        );
        assert_eq!(
            rendered(&mut run),
            [
                Some((String::from("Alpha"), 2, 9)),
                Some((String::from("Beta"), 2, 9)),
            ]
        );

        let mut revision = input.begin().unwrap();
        revision
            .insert(key("a.md"), document("a.md", "Changed"))
            .unwrap();
        revision.remove(key("b.md")).unwrap();
        let input = revision.seal().unwrap();
        let mut run = runner.settle().unwrap();
        assert_eq!(snapshot(&mut run), (vec![String::from("Changed")], 1, 7));
        assert_eq!(
            rendered(&mut run),
            [None, Some((String::from("Changed"), 1, 7))]
        );

        drop(input);
    }
}
