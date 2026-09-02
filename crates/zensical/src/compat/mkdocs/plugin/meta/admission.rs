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

//! Provider admission workaround for revision-local metadata facts.

use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, io};

use zrx::id::Id;
use zrx::stream::{Change, Key};

use crate::path::{SourcePath, SourceRoot};
use crate::watcher::Source;

use super::{claims, Index, Settings};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Provider-side metadata state retained while one workflow is serving.
///
/// This is the narrow admission workaround for the pinned runtime. It keeps
/// the immutable parsed index alive across unrelated source revisions and
/// rebuilds it only when the documentation metadata relation changes.
pub struct Admission {
    /// Documentation root used to resolve metadata descendants.
    docs: SourceRoot,
    /// Provider context whose metadata changes are admitted.
    context: String,
    /// Immutable settings shared with the metadata pipeline.
    settings: Arc<Settings>,
    /// Most recently prepared metadata index.
    index: Option<Arc<Index>>,
}

// ----------------------------------------------------------------------------

/// Metadata facts prepared for one provider revision.
pub struct Prepared {
    /// Immutable metadata index for the admitted revision.
    pub index: Arc<Index>,
    /// Descendant Markdown sources invalidated by metadata changes.
    pub dependents: Vec<(Key<Id>, Source)>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Admission {
    /// Creates metadata admission state for one workflow lifetime.
    pub fn new(
        docs: SourceRoot, context: String, settings: Arc<Settings>,
    ) -> Self {
        Self {
            docs,
            context,
            settings,
            index: None,
        }
    }

    /// Refreshes metadata when needed and returns dependent page insertions.
    pub fn prepare(
        &mut self, changes: &[Change<Id, Source>],
    ) -> Result<Prepared> {
        if self.index.is_none()
            || changes.iter().any(|change| self.claims(change_key(change)))
        {
            self.index =
                Some(Arc::new(Index::load(&self.docs, &self.settings)?));
        }
        let dependents = self.dependents(changes)?;
        Ok(Prepared {
            index: self
                .index
                .as_ref()
                .expect("metadata index initialized above")
                .clone(),
            dependents,
        })
    }

    /// Returns whether this source is a documentation metadata file.
    fn claims(&self, key: &Key<Id>) -> bool {
        key[0].context() == self.context
            && claims(&key[0].location(), &self.settings)
    }

    /// Expands metadata changes into descendant Markdown insertions.
    fn dependents(
        &self, changes: &[Change<Id, Source>],
    ) -> Result<Vec<(Key<Id>, Source)>> {
        if !self.settings.enabled {
            return Ok(Vec::new());
        }
        let mut dependents = BTreeMap::new();
        for change in changes {
            let key = change_key(change);
            if !self.claims(key) {
                continue;
            }
            let location = key[0].location().parse::<SourcePath>()?;
            let directory = location.parent().map_or_else(
                || self.docs.as_path().to_owned(),
                |parent| self.docs.join(&parent),
            );
            let mut paths = Vec::new();
            collect_markdown(&directory, &mut paths)?;
            for path in paths {
                let relative = path.strip_prefix(self.docs.as_path())?;
                let location = SourcePath::from_path(relative)?;
                let id = key[0]
                    .to_builder()
                    .location(location.as_str())
                    .build()
                    .expect("invariant");
                dependents.insert(Key::from(id), Source::from(path));
            }
        }

        // A provider update for the page itself is authoritative. In
        // particular, the initial snapshot contains both metadata files and
        // every Markdown page, so synthesized inserts must not admit each page
        // twice.
        for change in changes {
            dependents.remove(change_key(change));
        }
        Ok(dependents.into_iter().collect())
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns the resource key carried by one provider change.
fn change_key(change: &Change<Id, Source>) -> &Key<Id> {
    match change {
        Change::Insert(key, _) | Change::Remove(key) => key,
    }
}

/// Collects descendant Markdown source paths below one metadata directory.
fn collect_markdown(
    directory: &Path, paths: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_markdown(&path, paths)?;
        } else if path.extension().is_some_and(|extension| extension == "md") {
            paths.push(path);
        }
    }
    Ok(())
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use tempfile::tempdir;
    use zrx::id::Id;
    use zrx::stream::{Change, Key};

    use crate::path::SourceRoot;
    use crate::watcher::Source;

    use super::Admission;
    use super::Settings;

    #[test]
    fn selects_only_descendant_markdown() {
        let dir = tempdir().unwrap();
        let docs = dir.path();
        fs::create_dir_all(docs.join("guide/nested")).unwrap();
        fs::create_dir_all(docs.join("guidelines")).unwrap();
        fs::write(docs.join("guide/page.md"), "# Page").unwrap();
        fs::write(docs.join("guide/nested/page.md"), "# Nested").unwrap();
        fs::write(docs.join("guidelines/page.md"), "# Other").unwrap();
        let changes = vec![source_insert(
            "docs",
            "guide/.meta.yml",
            docs.join("guide/.meta.yml"),
        )];
        let mut metadata = admission(docs);

        let locations = metadata
            .prepare(&changes)
            .unwrap()
            .dependents
            .iter()
            .map(|(key, _)| key[0].location().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(locations, vec!["guide/nested/page.md", "guide/page.md"]);
    }

    #[test]
    fn provider_page_change_supersedes_metadata_dependent() {
        let dir = tempdir().unwrap();
        let docs = dir.path();
        fs::create_dir_all(docs.join("guide")).unwrap();
        let page = docs.join("guide/page.md");
        fs::write(&page, "# Page").unwrap();
        let changes = vec![
            source_insert(
                "docs",
                "guide/.meta.yml",
                docs.join("guide/.meta.yml"),
            ),
            source_insert("docs", "guide/page.md", page),
        ];
        let mut metadata = admission(docs);

        assert!(metadata.prepare(&changes).unwrap().dependents.is_empty());
    }

    #[test]
    fn reuses_index_until_a_docs_metadata_change() {
        let dir = tempdir().unwrap();
        let docs = dir.path();
        let path = docs.join(".meta.yml");
        fs::write(&path, "value: first\n").unwrap();
        let mut metadata = admission(docs);

        let initial = metadata.prepare(&[]).unwrap().index;
        let unrelated =
            vec![source_insert("docs", "asset.txt", docs.join("asset.txt"))];
        let reused = metadata.prepare(&unrelated).unwrap().index;
        assert!(Arc::ptr_eq(&initial, &reused));

        let theme = vec![source_insert(
            "templates/0",
            ".meta.yml",
            dir.path().join("theme/.meta.yml"),
        )];
        let still_reused = metadata.prepare(&theme).unwrap().index;
        assert!(Arc::ptr_eq(&initial, &still_reused));

        fs::write(&path, "value: second\n").unwrap();
        let changed = vec![source_insert("docs", ".meta.yml", path)];
        let refreshed = metadata.prepare(&changed).unwrap().index;
        assert!(!Arc::ptr_eq(&initial, &refreshed));
    }

    fn admission(docs: &Path) -> Admission {
        Admission::new(
            SourceRoot::open(docs).unwrap(),
            "docs".into(),
            Arc::new(Settings {
                enabled: true,
                meta_file: ".meta.yml".into(),
            }),
        )
    }

    fn source_insert(
        context: &str, location: &str, path: PathBuf,
    ) -> Change<Id, Source> {
        let id = Id::builder()
            .provider("file")
            .context(context)
            .location(location)
            .build()
            .unwrap();
        Change::Insert(Key::from(id), Source::from(path))
    }
}
