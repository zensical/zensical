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

//! MkDocs Material metadata inheritance.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::Hash;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::config::Config;
use crate::path::{SourcePath, SourceRoot};
use crate::structure::dynamic::Dynamic;

mod admission;
mod parser;

pub use admission::{Admission, Prepared};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Origin of one metadata value.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    /// Value read from a source document.
    Source(SourceSpan),
    /// Value created or changed during rendering.
    Runtime,
}

// ----------------------------------------------------------------------------

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

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MkDocs Material metadata pipeline.
#[derive(Clone, Debug)]
pub struct Meta {
    /// Immutable settings shared with provider admission and page resolution.
    settings: Arc<Settings>,
}

// ----------------------------------------------------------------------------

/// Inputs required to install metadata admission.
pub struct Dependencies {
    /// Documentation root used to resolve metadata descendants.
    pub docs: SourceRoot,
    /// Provider context whose metadata changes are admitted.
    pub context: String,
}

// ----------------------------------------------------------------------------

/// Metadata plugin settings used by the workflow.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Whether inheritance is enabled.
    pub enabled: bool,
    /// Exact basename of metadata files.
    pub meta_file: String,
}

// ----------------------------------------------------------------------------

/// A source range expressed as UTF-8 byte offsets.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    /// Source identifier.
    pub source: SourcePath,
    /// Half-open byte range within the complete source.
    pub range: Range<usize>,
}

// ----------------------------------------------------------------------------

/// A source-aware metadata value.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Origin of this node.
    origin: Origin,
    /// Value and source-aware children.
    value: Value,
}

// ----------------------------------------------------------------------------

/// One parsed YAML metadata document.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Source-relative path.
    path: SourcePath,
    /// Source-aware root mapping.
    root: Node,
}

// ----------------------------------------------------------------------------

/// Metadata resolved for one Markdown page.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    /// Source-aware root mapping.
    root: Node,
}

// ----------------------------------------------------------------------------

/// Immutable metadata documents available to one workflow revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Index {
    /// Parsed metadata files, shared by every page in the revision.
    documents: Vec<Document>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Meta {
    /// Resolves the private settings owned by this pipeline instance.
    pub fn new(config: &Config) -> Self {
        Self {
            settings: Arc::new(Settings::new(config)),
        }
    }

    /// Installs the provider-side metadata admission boundary.
    pub fn setup(&self, dependencies: Dependencies) -> Admission {
        Admission::new(
            dependencies.docs,
            dependencies.context,
            self.settings.clone(),
        )
    }

    /// Returns the immutable settings shared with resource classification.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

// ----------------------------------------------------------------------------

impl Settings {
    /// Extracts native meta settings from resolved configuration.
    pub fn new(config: &Config) -> Self {
        let config = &config.project.plugins.meta.config;
        Self {
            enabled: config.enabled,
            meta_file: config.meta_file.clone(),
        }
    }
}

// ----------------------------------------------------------------------------

impl Node {
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
}

// ----------------------------------------------------------------------------

impl Resolved {
    /// Returns plain values for the Python Markdown boundary.
    pub fn values(&self) -> BTreeMap<String, Dynamic> {
        let Value::Map(values) = &self.root.value else {
            unreachable!("metadata root is always a mapping")
        };
        values
            .iter()
            .map(|(key, value)| (key.clone(), value.dynamic()))
            .collect()
    }
}

// ----------------------------------------------------------------------------

impl Index {
    /// Loads and parses every configured metadata file exactly once.
    pub fn load(docs: &SourceRoot, settings: &Settings) -> Result<Self> {
        if !settings.enabled {
            return Ok(Self::default());
        }
        let mut documents = Vec::new();
        collect(docs, docs.as_path(), settings, &mut documents)?;
        sort_documents(&mut documents);
        Ok(Self { documents })
    }

    /// Resolves the metadata chain applicable to one page.
    pub fn resolve(
        &self, page: &SourcePath, front_matter: Option<Document>,
    ) -> Result<Resolved> {
        resolve_ordered(
            self.documents
                .iter()
                .filter(|document| applies(&document.path, page)),
            front_matter,
        )
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns whether a source is claimed as a metadata file.
pub fn claims(path: &str, settings: &Settings) -> bool {
    settings.enabled
        && path.rsplit('/').next() == Some(settings.meta_file.as_str())
}

/// Returns whether a metadata file applies to a Markdown page.
pub fn applies(meta: &SourcePath, page: &SourcePath) -> bool {
    meta.parent()
        .is_none_or(|parent| page.is_descendant_of(&parent))
}

/// Parses one standalone metadata file.
pub fn parse(path: SourcePath, source: &str) -> Result<Document> {
    parser::parse(path.clone(), source, 0)
        .with_context(|| format!("error reading meta file '{path}'"))
}

/// Extracts and parses YAML front matter from a Markdown source.
pub fn front_matter(
    path: &SourcePath, source: &str,
) -> Result<(String, Option<Document>)> {
    parser::front_matter(path, source)
        .with_context(|| format!("error reading page metadata '{path}'"))
}

/// Recursively loads metadata documents from the docs tree.
fn collect(
    root: &SourceRoot, directory: &Path, settings: &Settings,
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
            let relative = path.strip_prefix(root.as_path())?;
            let location = SourcePath::from_path(relative)?;
            let source = std::fs::read_to_string(&path)?;
            documents.push(parse(location, &source)?);
        }
    }
    Ok(())
}

/// Sorts metadata from broad ancestors to specific descendants once.
fn sort_documents(documents: &mut [Document]) {
    documents.sort_by(|left, right| {
        let left_depth = left.path.depth();
        let right_depth = right.path.depth();
        left_depth
            .cmp(&right_depth)
            .then(left.path.cmp(&right.path))
    });
}

/// Resolves metadata whose source order is already deterministic.
fn resolve_ordered<'a>(
    documents: impl IntoIterator<Item = &'a Document>, page: Option<Document>,
) -> Result<Resolved> {
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

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use anyhow::Result;

    use crate::path::{SourcePath, SourceRoot};
    use crate::structure::dynamic::Dynamic;

    use super::{
        applies, parse, resolve_ordered, sort_documents, Document, Index,
        Origin, Resolved, Settings, Value,
    };

    fn resolve(
        documents: impl IntoIterator<Item = Document>, page: Option<Document>,
    ) -> Result<Resolved> {
        let mut documents = documents.into_iter().collect::<Vec<_>>();
        sort_documents(&mut documents);
        resolve_ordered(&documents, page)
    }

    fn values(source: &str) -> BTreeMap<String, Dynamic> {
        resolve([], Some(parse(path("docs/page.md"), source).unwrap()))
            .unwrap()
            .values()
    }

    fn path(value: &str) -> SourcePath {
        value.parse().unwrap()
    }

    #[test]
    fn matches_path_components() {
        assert!(applies(
            &path("docs/guide/.meta.yml"),
            &path("docs/guide/page.md")
        ));
        assert!(!applies(
            &path("docs/guide/.meta.yml"),
            &path("docs/guidelines/page.md")
        ));
        assert!(applies(
            &path("docs/café/defaults.yml"),
            &path("docs/café/nested/page.md")
        ));
        assert!(!applies(
            &path("docs/café/defaults.yml"),
            &path("docs/café-other/page.md")
        ));
    }

    #[test]
    fn parses_null_as_null() {
        assert_eq!(values("value: null\n")["value"], Dynamic::Null);
    }

    #[test]
    fn merges_maps_and_appends_lists() {
        let root =
            parse(path("docs/.meta.yml"), "x:\n  a: 1\nitems: [a]\n").unwrap();
        let nested =
            parse(path("docs/guide/.meta.yml"), "x:\n  b: 2\nitems: [b]\n")
                .unwrap();
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
        let root = parse(path("docs/.meta.yml"), "value: text\n").unwrap();
        let page = parse(path("docs/page.md"), "value: [text]\n").unwrap();
        let error = format!("{:#}", resolve([root], Some(page)).unwrap_err());
        assert!(error.contains(".meta.yml"));
        assert!(error.contains("page.md"));
    }

    #[test]
    fn scalar_override_keeps_winning_source() {
        let root = parse(path(".meta.yml"), "title: Default\n").unwrap();
        let page = parse(path("page.md"), "title: Page\n").unwrap();
        let resolved = resolve([root], Some(page)).unwrap();
        let Value::Map(values) = &resolved.root.value else {
            panic!("mapping")
        };
        let Origin::Source(span) = &values["title"].origin else {
            panic!("source")
        };
        assert_eq!(span.source.as_str(), "page.md");
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
        let root = SourceRoot::open(docs).unwrap();
        let values = Index::load(&root, &settings)
            .unwrap()
            .resolve(&path("guide/page.md"), None)
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
