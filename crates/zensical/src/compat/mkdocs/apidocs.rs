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

//! Shared generated sources, source tracking, and navigation for API plugins.
//!
//! A build owns one immutable snapshot. Generated sources enter the normal
//! document relation; source changes restart discovery like mkdocstrings.

use anyhow::{Context, Result};
use globset::GlobMatcher;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zrx::id::Id;
use zrx::stream::Change;

use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::nav::NavigationItem;
use crate::watcher::Source;

mod nav;
pub(super) use nav::Node;

/// One generated source and its optional repository edit target.
#[derive(Clone, Debug)]
pub struct Document {
    /// Complete Markdown content.
    pub content: Arc<str>,
    /// Original source file for AutoAPI edit links; autonav pages have none.
    pub edit: Option<PathBuf>,
    /// Whether this is a navigation control document.
    pub control: bool,
}

/// Immutable generated files and source dependencies for a build.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Sources keyed by their ordinary documentation identities.
    pub files: BTreeMap<SourcePath, Document>,
    /// Source directories monitored for additions as well as changes.
    pub roots: Vec<PathBuf>,
    /// Initial source content hashes, also used to invalidate render caches.
    pub observed: BTreeMap<PathBuf, u64>,
    pub(super) sections: Vec<Section>,
    pub(super) patterns: Vec<GlobMatcher>,
    pub(super) ignored_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub(super) struct Section {
    pub(super) root: String,
    pub(super) title: Option<String>,
    pub(super) children: Vec<NavigationItem>,
    pub(super) autonav: bool,
    pub(super) nav_item_prefix: String,
    pub(super) show_full_namespace: bool,
    pub(super) generated: bool,
}

impl Snapshot {
    /// Discovers both plugins before their sources enter the build workflow.
    pub fn new(config: &Config, strict: bool) -> Result<Self> {
        if !config.project.plugins.autoapi.config.enabled
            && !config.project.plugins.api_autonav.config.enabled
        {
            return Ok(Self::default());
        }
        let mut snapshot = Self {
            ignored_roots: vec![
                config.output_root().as_path().to_owned(),
                config.get_cache_dir(),
            ],
            ..Self::default()
        };
        super::plugin::autoapi::generate(config, &mut snapshot)?;
        super::plugin::api_autonav::generate(config, strict, &mut snapshot)?;
        snapshot.roots.sort();
        snapshot.roots.dedup();
        Ok(snapshot)
    }

    pub(super) fn add(
        &mut self, path: &str, content: String, edit: Option<PathBuf>,
        control: bool,
    ) -> Result<()> {
        let path: SourcePath = path.parse()?;
        // Like MkDocs' generated Files collection, later modules replace an
        // existing source at the same path (including AutoAPI's index.py).
        self.files.insert(
            path,
            Document {
                content: content.into(),
                edit,
                control,
            },
        );
        Ok(())
    }

    pub(super) fn observe(&mut self, path: &Path) -> Result<()> {
        if self.is_source(path) {
            self.observed
                .insert(path.to_owned(), fingerprint(&fs::read(path)?));
        }
        Ok(())
    }

    /// Whether a file event can change generated documentation.
    pub fn is_source(&self, path: &Path) -> bool {
        if self.ignored_roots.iter().any(|root| path.starts_with(root))
            || ignored(path)
        {
            return false;
        }
        self.roots.iter().any(|root| path.starts_with(root))
            && (path.extension().is_some_and(|extension| {
                extension == "py" || extension == "pyi"
            }) || self
                .patterns
                .iter()
                .any(|pattern| pattern.is_match(path)))
    }

    /// Detects edits, additions and removals against the discovery snapshot.
    pub fn changed(&self, path: &Path) -> bool {
        self.is_source(path)
            && fs::read(path).ok().map(|content| fingerprint(&content))
                != self.observed.get(path).copied()
    }

    /// Initial generated sources, sharing identities with physical documents.
    pub fn changes(&self, config: &Config) -> Vec<Change<Id, Source>> {
        self.files
            .iter()
            .map(|(path, document)| {
                let id = Id::builder()
                    .provider("file")
                    .context(&config.project.docs_dir)
                    .location(path.as_str())
                    .build()
                    .expect("valid source identity");
                Change::Insert(
                    id.into(),
                    Source::generated(
                        config.docs_root().join(path),
                        document.content.clone(),
                    ),
                )
            })
            .collect()
    }

    /// True for a generated navigation control file, which is not a page.
    pub fn is_control(&self, path: &str) -> bool {
        path.parse::<SourcePath>()
            .ok()
            .and_then(|path| self.files.get(&path))
            .is_some_and(|document| document.control)
    }
}

pub(super) fn module_path(
    relative: &Path, root: &str, escape_index: bool,
) -> Result<(Vec<String>, String)> {
    let mut path = relative.with_extension("md");
    let stem = relative.with_extension("");
    let mut parts = stem
        .iter()
        .map(|part| {
            part.to_str()
                .context("module path must be UTF-8")
                .map(str::to_owned)
        })
        .collect::<Result<Vec<_>>>()?;
    if parts.last().is_some_and(|part| part == "__init__") {
        parts.pop();
        path.set_file_name("index.md");
    } else if escape_index && parts.last().is_some_and(|part| part == "index") {
        path.set_file_name("index_py.md");
    }
    Ok((
        parts,
        format!("{root}/{}", path.to_string_lossy().replace('\\', "/")),
    ))
}

fn ignored(path: &Path) -> bool {
    path.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some(".git" | ".venv" | "venv" | "__pycache__")
        )
    })
}

pub(super) fn walk(
    root: &Path, ignored_roots: &[PathBuf], files: &mut Vec<PathBuf>,
) -> Result<()> {
    if ignored(root) || ignored_roots.iter().any(|path| root.starts_with(path))
    {
        return Ok(());
    }
    if root.is_file() {
        files.push(root.to_owned());
        return Ok(());
    }
    let mut entries = fs::read_dir(root)
        .with_context(|| {
            format!("cannot discover API sources in {}", root.display())
        })?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(&entry.path(), ignored_roots, files)?;
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn fingerprint(content: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn under(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::{module_path, Snapshot};
    use std::{fs, path::Path};

    #[test]
    fn module_paths_preserve_packages_and_escape_index_modules() {
        assert_eq!(
            module_path(Path::new("pkg/__init__.py"), "api", true).unwrap(),
            (vec!["pkg".to_owned()], "api/pkg/index.md".to_owned())
        );
        assert_eq!(
            module_path(Path::new("pkg/index.py"), "api", true).unwrap(),
            (
                vec!["pkg".to_owned(), "index".to_owned()],
                "api/pkg/index_py.md".to_owned()
            )
        );
        assert_eq!(
            module_path(Path::new("pkg/sub/mod.pyi"), "api", false)
                .unwrap()
                .1,
            "api/pkg/sub/mod.md"
        );
    }

    #[test]
    fn dependency_snapshot_tracks_creation_edits_removal_and_virtualenvs() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("module.py");
        fs::write(&file, "old").unwrap();
        let mut snapshot = Snapshot {
            roots: vec![directory.path().to_owned()],
            ..Snapshot::default()
        };
        snapshot.observe(&file).unwrap();
        assert!(!snapshot.changed(&file));
        fs::write(&file, "new").unwrap();
        assert!(snapshot.changed(&file));
        fs::remove_file(&file).unwrap();
        assert!(snapshot.changed(&file));
        let added = directory.path().join("new.py");
        fs::write(&added, "added").unwrap();
        assert!(snapshot.changed(&added));
        assert!(
            !snapshot.is_source(&directory.path().join(".venv/lib/module.py"))
        );
    }
}
