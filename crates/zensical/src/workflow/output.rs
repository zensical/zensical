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

//! Page-output ownership and reconciliation.

use anyhow::{anyhow, bail};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use zrx::id::Id;
use zrx::scheduler::action::{Action, Concurrency, Context};
use zrx::stream::function::Collection;
use zrx::stream::operator::Operator;
use zrx::stream::{Change, Key, Stream, Value};

#[cfg(test)]
use crate::path::SourcePath;
use crate::path::{OutputRoot, SitePath};
use crate::structure::page::PageOrigin;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One rendered page claiming a site-relative destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// Site-relative output path.
    pub destination: SitePath,
    /// Complete rendered bytes.
    pub contents: Arc<Vec<u8>>,
    /// Logical producer used for arbitration and diagnostics.
    pub owner: PageOrigin,
}

/// Writes effective insertions and removes retracted page outputs.
#[derive(Clone)]
struct Writer {
    output: OutputRoot,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Artifact {
    /// Creates an ordinary source-owned page artifact.
    #[cfg(test)]
    pub fn source(
        source: SourcePath, destination: SitePath, contents: Vec<u8>,
    ) -> Self {
        Self {
            destination,
            contents: Arc::new(contents),
            owner: PageOrigin::Source(source),
        }
    }

    /// Creates an artifact owned by an already constructed page.
    pub fn page(
        owner: PageOrigin, destination: SitePath, contents: Vec<u8>,
    ) -> Self {
        Self {
            destination,
            contents: Arc::new(contents),
            owner,
        }
    }
}

impl Writer {
    fn path(&self, key: &Key<Id>) -> anyhow::Result<PathBuf> {
        let id = key.try_as_id()?;
        if id.context() != "." {
            return Err(anyhow!("page output escaped the site directory"));
        }
        let path = id.location().parse::<SitePath>()?;
        Ok(self.output.join(&path))
    }

    fn insert(&self, key: &Key<Id>, artifact: &Artifact) -> anyhow::Result<()> {
        let path = self.path(key)?;
        fs::create_dir_all(path.parent().expect("site page has parent"))?;
        fs::write(path, artifact.contents.as_slice())?;
        Ok(())
    }

    fn remove(&self, key: &Key<Id>) -> anyhow::Result<()> {
        let path = self.path(key)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Artifact {}

impl Action<Key<Id>> for Writer {
    type Inputs = (Artifact,);
    type Output = ();

    fn concurrency(&self) -> Concurrency<Self> {
        Concurrency::adaptive()
    }

    fn execute(&mut self, context: Context<'_, Key<Id>, Self>) {
        let Context { inputs: input, output, .. } = context;
        input.for_each(output, |change, emit| {
            match change {
                Change::Insert(key, artifact) => {
                    self.insert(&key, artifact.as_ref())?;
                    emit.insert(key, ());
                }
                Change::Remove(key) => {
                    self.remove(&key)?;
                    emit.remove(key);
                }
            }
            Ok(())
        });
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Resolves page ownership by destination and installs the retained writer.
pub fn setup(output: OutputRoot, artifacts: &Stream<Id, Artifact>) {
    let outputs = artifacts.reduce_by_key(
        |artifact: &Artifact| output_key(&artifact.destination),
        |claims: &dyn Collection<Key<Id>, Artifact>| preferred(claims.values()),
    );
    let _ = outputs.subscribe(Writer { output });
}

/// Creates the destination identity used by the output relation.
fn output_key(path: &SitePath) -> anyhow::Result<Key<Id>> {
    let id = Id::builder()
        .provider("page")
        .context(".")
        .location(path.as_str())
        .build()?;
    Ok(Key::from(id))
}

/// Selects one effective claim or rejects ambiguous ownership.
fn preferred<'a>(
    claims: impl Iterator<Item = &'a Artifact>,
) -> anyhow::Result<Option<Artifact>> {
    let mut claims = claims.collect::<Vec<_>>();
    claims.sort_by(|left, right| left.owner.cmp(&right.owner));
    match claims.as_slice() {
        [] => Ok(None),
        [artifact] => Ok(Some((*artifact).clone())),
        [left, right] if index_readme_pair(&left.owner, &right.owner) => {
            Ok(claims
                .into_iter()
                .find(|artifact| {
                    source_name(&artifact.owner) == Some("index.md")
                })
                .cloned())
        }
        _ => {
            let destination = &claims[0].destination;
            let owners = claims
                .iter()
                .map(|artifact| owner_name(&artifact.owner))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("page destination '{destination}' has multiple owners: {owners}")
        }
    }
}

/// Returns whether two claims are MkDocs' index-over-README source pair.
fn index_readme_pair(left: &PageOrigin, right: &PageOrigin) -> bool {
    matches!(
        (source_name(left), source_name(right)),
        (Some("README.md"), Some("index.md"))
            | (Some("index.md"), Some("README.md"))
    )
}

fn source_name(owner: &PageOrigin) -> Option<&str> {
    match owner {
        PageOrigin::Source(source) => Some(source.file_name()),
        PageOrigin::Generated { .. } => None,
    }
}

fn owner_name(owner: &PageOrigin) -> String {
    match owner {
        PageOrigin::Source(source) => format!("source '{source}'"),
        PageOrigin::Generated { identity, .. } => {
            format!("generated '{identity}'")
        }
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use zrx::id::Id;
    use zrx::stream::{Key, Workflow};

    use super::{output_key, preferred, Artifact, Writer};
    use crate::path::OutputRoot;
    use crate::structure::page::PageOrigin;

    fn source(source: &str, contents: &str) -> Artifact {
        Artifact::source(
            source.parse().unwrap(),
            "index.html".parse().unwrap(),
            contents.as_bytes().to_vec(),
        )
    }

    #[test]
    fn one_owner_is_selected() {
        let artifact = source("index.md", "index");
        assert_eq!(preferred([&artifact].into_iter()).unwrap(), Some(artifact));
    }

    #[test]
    fn index_takes_precedence_over_readme() {
        let index = source("index.md", "index");
        let readme = source("README.md", "readme");
        assert_eq!(
            preferred([&readme, &index].into_iter()).unwrap(),
            Some(index)
        );
    }

    #[test]
    fn ambiguous_sources_are_rejected_deterministically() {
        let left = source("one.md", "one");
        let right = source("two.md", "two");
        let error = preferred([&right, &left].into_iter()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "page destination 'index.html' has multiple owners: source 'one.md', source 'two.md'"
        );
    }

    #[test]
    fn generated_and_source_ownership_is_ambiguous() {
        let source = source("index.md", "source");
        let generated = Artifact {
            destination: "index.html".parse().unwrap(),
            contents: std::sync::Arc::default(),
            owner: PageOrigin::Generated {
                identity: "blog:index".into(),
                provenance: None,
            },
        };
        assert!(preferred([&source, &generated].into_iter()).is_err());
    }

    #[test]
    fn writer_retracts_removed_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let writer = Writer {
            output: OutputRoot::prepare(directory.path()).unwrap(),
        };
        let artifact = Artifact::source(
            "guide.md".parse().unwrap(),
            "guide/index.html".parse().unwrap(),
            b"guide".to_vec(),
        );
        let key = output_key(&artifact.destination).unwrap();

        writer.insert(&key, &artifact).unwrap();
        let path = directory.path().join("guide/index.html");
        assert_eq!(std::fs::read(&path).unwrap(), b"guide");

        writer.remove(&key).unwrap();
        assert!(!path.exists());
        writer.remove(&key).unwrap();
    }

    #[test]
    fn writer_reconciles_route_changes() {
        let directory = tempfile::tempdir().unwrap();
        let writer = Writer {
            output: OutputRoot::prepare(directory.path()).unwrap(),
        };
        let old = Artifact::source(
            "post.md".parse().unwrap(),
            "old/index.html".parse().unwrap(),
            b"old".to_vec(),
        );
        let new = Artifact::source(
            "post.md".parse().unwrap(),
            "new/index.html".parse().unwrap(),
            b"new".to_vec(),
        );
        let old_key = output_key(&old.destination).unwrap();
        let new_key = output_key(&new.destination).unwrap();

        writer.insert(&old_key, &old).unwrap();
        writer.remove(&old_key).unwrap();
        writer.insert(&new_key, &new).unwrap();

        assert!(!directory.path().join("old/index.html").exists());
        assert_eq!(
            std::fs::read(directory.path().join("new/index.html")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn retained_ownership_reconciles_moves_handoffs_and_removal() {
        let directory = tempfile::tempdir().unwrap();
        let root = OutputRoot::prepare(directory.path()).unwrap();
        let workflow = Workflow::<Id>::build(|workflow| {
            let artifacts = workflow.input::<Artifact>();
            super::setup(root, &artifacts);
        });
        let mut runner = workflow.runner().unwrap();
        let input = runner.input::<Artifact>().unwrap();
        let source_key = Key::from(
            Id::builder()
                .provider("test")
                .context(".")
                .location("page")
                .build()
                .unwrap(),
        );

        let mut revision = input.begin().unwrap();
        revision
            .insert(
                source_key.clone(),
                Artifact::page(
                    PageOrigin::Generated {
                        identity: "blog:main:2".into(),
                        provenance: None,
                    },
                    "old/index.html".parse().unwrap(),
                    b"old".to_vec(),
                ),
            )
            .unwrap();
        let mut input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("old/index.html")).unwrap(),
            b"old"
        );

        let mut revision = input.begin().unwrap();
        revision
            .insert(
                source_key.clone(),
                Artifact::page(
                    PageOrigin::Generated {
                        identity: "blog:main:2".into(),
                        provenance: None,
                    },
                    "new/index.html".parse().unwrap(),
                    b"generated".to_vec(),
                ),
            )
            .unwrap();
        input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert!(!directory.path().join("old/index.html").exists());
        assert_eq!(
            std::fs::read(directory.path().join("new/index.html")).unwrap(),
            b"generated"
        );

        let mut revision = input.begin().unwrap();
        revision
            .insert(
                source_key.clone(),
                Artifact::page(
                    PageOrigin::Source("new.md".parse().unwrap()),
                    "new/index.html".parse().unwrap(),
                    b"source".to_vec(),
                ),
            )
            .unwrap();
        input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("new/index.html")).unwrap(),
            b"source"
        );

        let mut revision = input.begin().unwrap();
        revision.remove(source_key).unwrap();
        input = revision.seal().unwrap();
        let _run = runner.settle().unwrap();
        assert!(!directory.path().join("new/index.html").exists());
        drop(input);
    }
}
