// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! MkDocs Material metadata inheritance.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::Hash;
use std::ops::Range;
use std::path::Path;

use crate::config::Config;
use crate::structure::dynamic::Dynamic;

mod parser;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Metadata plugin settings used by the workflow.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    /// Whether inheritance is enabled.
    pub enabled: bool,
    /// Exact basename of metadata files.
    pub meta_file: String,
}

/// A source range expressed as UTF-8 byte offsets.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SourceSpan {
    /// Source identifier.
    pub source: String,
    /// Half-open byte range within the complete source.
    pub range: Range<usize>,
}

/// Origin of one metadata value.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Origin {
    /// Value read from a source document.
    Source(SourceSpan),
    /// Value created or changed during rendering.
    Runtime,
}

/// A source-aware metadata value.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Node {
    /// Origin of this node.
    origin: Origin,
    /// Value and source-aware children.
    value: Value,
}

/// Recursive metadata value.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
enum Value {
    /// Scalar value.
    Scalar(Dynamic),
    /// Sequence value.
    List(Vec<Node>),
    /// Mapping value.
    Map(BTreeMap<String, Node>),
}

/// One parsed YAML metadata document.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Document {
    /// Source-relative path.
    path: String,
    /// Source-aware root mapping.
    root: Node,
}

/// Metadata resolved for one Markdown page.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Resolved {
    /// Source-aware root mapping.
    root: Node,
}

/// Immutable metadata documents available to one workflow revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Index {
    /// Parsed metadata files, shared by every page in the revision.
    documents: Vec<Document>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Settings {
    /// Extracts native meta settings from resolved configuration.
    pub(crate) fn new(config: &Config) -> Self {
        let config = &config.project.plugins.meta.config;
        Self {
            enabled: config.enabled,
            meta_file: config.meta_file.clone(),
        }
    }
}

impl Node {
    /// Creates a runtime-owned tree from a plain dynamic value.
    fn runtime(value: Dynamic) -> Self {
        let value = match value {
            Dynamic::List(values) => {
                Value::List(values.into_iter().map(Self::runtime).collect())
            }
            Dynamic::Map(values) => Value::Map(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::runtime(value)))
                    .collect(),
            ),
            value => Value::Scalar(value),
        };
        Self { origin: Origin::Runtime, value }
    }

    /// Projects a source-aware tree into template-visible metadata.
    fn dynamic(&self) -> Dynamic {
        match &self.value {
            Value::Scalar(value) => value.clone(),
            Value::List(values) => {
                Dynamic::List(values.iter().map(Self::dynamic).collect())
            }
            Value::Map(values) => Dynamic::Map(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.dynamic()))
                    .collect(),
            ),
        }
    }

    /// Retains origins for values unchanged by Python extensions.
    fn reconcile(&self, value: Dynamic) -> Self {
        if self.dynamic() == value {
            return self.clone();
        }
        match (&self.value, value) {
            (Value::Map(previous), Dynamic::Map(current)) => {
                let values = current
                    .into_iter()
                    .map(|(key, value)| {
                        let value = if let Some(previous) = previous.get(&key) {
                            previous.reconcile(value)
                        } else {
                            Self::runtime(value)
                        };
                        (key, value)
                    })
                    .collect();
                Self {
                    origin: Origin::Runtime,
                    value: Value::Map(values),
                }
            }
            (Value::List(previous), Dynamic::List(current)) => {
                let values = current
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        if let Some(previous) = previous.get(index) {
                            previous.reconcile(value)
                        } else {
                            Self::runtime(value)
                        }
                    })
                    .collect();
                Self {
                    origin: Origin::Runtime,
                    value: Value::List(values),
                }
            }
            (_, value) => Self::runtime(value),
        }
    }
}

impl Resolved {
    /// Returns plain values for the Python Markdown boundary.
    pub(crate) fn values(&self) -> BTreeMap<String, Dynamic> {
        let Value::Map(values) = &self.root.value else {
            unreachable!("metadata root is always a mapping")
        };
        values
            .iter()
            .map(|(key, value)| (key.clone(), value.dynamic()))
            .collect()
    }

    /// Reconciles source origins with metadata returned from Python.
    pub(crate) fn reconcile(&self, values: BTreeMap<String, Dynamic>) -> Self {
        let root = self.root.reconcile(Dynamic::Map(values));
        Self { root }
    }
}

impl Index {
    /// Loads and parses every configured metadata file exactly once.
    pub(crate) fn load(docs: &Path, settings: &Settings) -> Result<Self> {
        if !settings.enabled {
            return Ok(Self::default());
        }
        let mut documents = Vec::new();
        collect(docs, docs, settings, &mut documents)?;
        Ok(Self { documents })
    }

    /// Resolves the metadata chain applicable to one page.
    pub(crate) fn resolve(
        &self, page: &str, front_matter: Option<Document>,
    ) -> Result<Resolved> {
        resolve(
            self.documents
                .iter()
                .filter(|document| applies(&document.path, page))
                .cloned(),
            front_matter,
        )
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns whether a source is claimed as a metadata file.
pub(crate) fn claims(path: &str, settings: &Settings) -> bool {
    settings.enabled
        && path.rsplit('/').next() == Some(settings.meta_file.as_str())
}

/// Returns whether a metadata file applies to a Markdown page.
pub(crate) fn applies(meta: &str, page: &str) -> bool {
    let parent = meta.rsplit_once('/').map_or("", |(parent, _)| parent);
    parent.is_empty()
        || page
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Parses one standalone metadata file.
pub(crate) fn parse(path: &str, source: &str) -> Result<Document> {
    parser::parse(path, source, 0)
        .with_context(|| format!("error reading meta file '{path}'"))
}

/// Extracts and parses YAML front matter from a Markdown source.
pub(crate) fn front_matter(
    path: &str, source: &str,
) -> Result<(String, Option<Document>)> {
    parser::front_matter(path, source)
        .with_context(|| format!("error reading page metadata '{path}'"))
}

/// Recursively loads metadata documents from the docs tree.
fn collect(
    root: &Path, directory: &Path, settings: &Settings,
    documents: &mut Vec<Document>,
) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, settings, documents)?;
        } else if path
            .file_name()
            .is_some_and(|name| name == settings.meta_file.as_str())
        {
            let relative = path.strip_prefix(root)?;
            let location = relative.to_string_lossy().replace('\\', "/");
            let source = std::fs::read_to_string(&path)?;
            documents.push(parse(&location, &source)?);
        }
    }
    Ok(())
}

/// Resolves applicable meta files and page front matter.
pub(crate) fn resolve(
    documents: impl IntoIterator<Item = Document>, page: Option<Document>,
) -> Result<Resolved> {
    let mut documents = documents.into_iter().collect::<Vec<_>>();
    documents.sort_by(|left, right| {
        let left_depth = left.path.matches('/').count();
        let right_depth = right.path.matches('/').count();
        left_depth
            .cmp(&right_depth)
            .then(left.path.cmp(&right.path))
    });

    let mut root = Node {
        origin: Origin::Runtime,
        value: Value::Map(BTreeMap::new()),
    };
    for document in documents {
        merge(&mut root, document.root.clone()).with_context(|| {
            format!("error merging meta file '{}'", document.path)
        })?;
    }
    if let Some(page) = page {
        merge(&mut root, page.root).context("error merging page metadata")?;
    }
    Ok(Resolved { root })
}

/// Applies Material's typesafe-additive merge strategy.
fn merge(target: &mut Node, incoming: Node) -> Result<()> {
    let incoming_origin = incoming.origin.clone();
    match (&mut target.value, incoming.value) {
        (Value::Map(target), Value::Map(incoming)) => {
            for (key, value) in incoming {
                if let Some(current) = target.get_mut(&key) {
                    merge(current, value)?;
                } else {
                    target.insert(key, value);
                }
            }
            Ok(())
        }
        (Value::List(target), Value::List(mut incoming)) => {
            target.append(&mut incoming);
            Ok(())
        }
        (Value::Scalar(target_value), Value::Scalar(incoming))
            if scalar_kind(target_value) == scalar_kind(&incoming) =>
        {
            *target_value = incoming;
            target.origin = incoming_origin;
            Ok(())
        }
        _ => bail!(
            "metadata types do not match ({} conflicts with {})",
            origin_label(&target.origin),
            origin_label(&incoming_origin)
        ),
    }
}

/// Formats an origin for a concise merge diagnostic.
fn origin_label(origin: &Origin) -> String {
    match origin {
        Origin::Source(span) => {
            format!("{}:{}..{}", span.source, span.range.start, span.range.end)
        }
        Origin::Runtime => "runtime metadata".into(),
    }
}

/// Returns an exact scalar kind for typesafe replacement.
fn scalar_kind(value: &Dynamic) -> u8 {
    match value {
        Dynamic::Null => 0,
        Dynamic::String(_) => 1,
        Dynamic::Bool(_) => 2,
        Dynamic::Integer(_) => 3,
        Dynamic::Float(_) => 4,
        Dynamic::List(_) | Dynamic::Map(_) => unreachable!("nested value"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(source: &str) -> BTreeMap<String, Dynamic> {
        resolve([], Some(parse("docs/page.md", source).unwrap()))
            .unwrap()
            .values()
    }

    #[test]
    fn matches_path_components() {
        assert!(applies("docs/guide/.meta.yml", "docs/guide/page.md"));
        assert!(!applies("docs/guide/.meta.yml", "docs/guidelines/page.md"));
    }

    #[test]
    fn parses_null_as_null() {
        assert_eq!(values("value: null\n")["value"], Dynamic::Null);
    }

    #[test]
    fn merges_maps_and_appends_lists() {
        let root = parse("docs/.meta.yml", "x:\n  a: 1\nitems: [a]\n").unwrap();
        let nested =
            parse("docs/guide/.meta.yml", "x:\n  b: 2\nitems: [b]\n").unwrap();
        let values = resolve([nested, root], None).unwrap().values();
        assert_eq!(
            values["items"],
            Dynamic::List(vec![
                Dynamic::String("a".into()),
                Dynamic::String("b".into())
            ])
        );
        assert_eq!(
            values["x"],
            Dynamic::Map(BTreeMap::from([
                ("a".into(), Dynamic::Integer(1)),
                ("b".into(), Dynamic::Integer(2)),
            ]))
        );
    }

    #[test]
    fn rejects_type_mismatch() {
        let root = parse("docs/.meta.yml", "value: text\n").unwrap();
        let page = parse("docs/page.md", "value: [text]\n").unwrap();
        let error = format!("{:#}", resolve([root], Some(page)).unwrap_err());
        assert!(error.contains(".meta.yml"));
        assert!(error.contains("page.md"));
    }

    #[test]
    fn scalar_override_keeps_winning_source() {
        let root = parse(".meta.yml", "title: Default\n").unwrap();
        let page = parse("page.md", "title: Page\n").unwrap();
        let resolved = resolve([root], Some(page)).unwrap();
        let Value::Map(values) = &resolved.root.value else {
            panic!("mapping")
        };
        let Origin::Source(span) = &values["title"].origin else {
            panic!("source")
        };
        assert_eq!(span.source, "page.md");
    }

    #[test]
    fn loads_only_component_ancestors() {
        let directory = tempfile::tempdir().unwrap();
        let docs = directory.path();
        std::fs::create_dir_all(docs.join("guide")).unwrap();
        std::fs::create_dir_all(docs.join("guidelines")).unwrap();
        std::fs::write(docs.join(".meta.yml"), "items: [root]\n").unwrap();
        std::fs::write(docs.join("guide/.meta.yml"), "items: [guide]\n")
            .unwrap();
        std::fs::write(
            docs.join("guidelines/.meta.yml"),
            "items: [guidelines]\n",
        )
        .unwrap();
        let settings = Settings {
            enabled: true,
            meta_file: ".meta.yml".into(),
        };
        let values = Index::load(docs, &settings)
            .unwrap()
            .resolve("guide/page.md", None)
            .unwrap()
            .values();
        assert_eq!(
            values["items"],
            Dynamic::List(vec![
                Dynamic::String("root".into()),
                Dynamic::String("guide".into()),
            ])
        );
    }
}
