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

//! File watcher.

use crossbeam::channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use mio::Waker;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use zensical_watch::agent::Manager;
use zensical_watch::event::{Event, Kind};
use zensical_watch::{Agent, Error, Result};
use zrx::id::Id;
use zrx::stream::Change;

use crate::config::Config;
use crate::path::SourcePath;

mod source;

pub use source::Source;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// File watcher.
///
/// This is a thin wrapper around the file agent. We're going to refactor this
/// logic into a provider architecture that will make things more flexible.
pub struct Watcher {
    /// File agent.
    _agent: Agent,
    /// Debounced source changes.
    changes: Receiver<Batch>,
}

/// Source changes and initial discovery completion are delivered together.
pub struct Batch {
    /// Debounced source changes.
    pub changes: Vec<Change<Id, Source>>,
    /// The initial documentation scan is completed by this batch.
    pub sources_ready: bool,
}

/// One physical source root and its provider-relative identity context.
struct SourceMount {
    root: PathBuf,
    context: String,
    fallback: bool,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Watcher {
    /// Creates a file watcher.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        config: &Config, serve: bool, reload: Sender<String>,
        waker: Option<Arc<Waker>>,
    ) -> Result<Self> {
        let (changes, receiver) = unbounded();
        let mut sources = Vec::default();

        // Admit generated sources with the initial physical source revision.
        // They share docs identities; physical collisions are masked below.
        let mut generated = config.api.changes(config);

        // Add docs directory and theme directories
        sources.push(SourceMount::new(
            config.docs_root().as_path().to_owned(),
            config.project.docs_dir.clone(),
        ));
        for (i, theme_dir) in config.theme_dirs.iter().enumerate() {
            sources.push(SourceMount::new(
                theme_dir.clone(),
                format!("templates/{i}"),
            ));
        }

        // Resolve custom social layout directories before source mounts are
        // finalized. They may live outside the project root, in which case
        // they need their own provider-relative identity context.
        let project_root =
            config.path.parent().expect("configuration has parent");
        let social_layout_dirs = config
            .project
            .plugins
            .social
            .config
            .iter()
            .filter(|plugin| plugin.config.enabled)
            .map(|plugin| {
                let path = Path::new(&plugin.config.cards_layout_dir);
                let path = if path.is_absolute() {
                    path.to_owned()
                } else {
                    project_root.join(path)
                };
                canonical_or_clone(&path)
            })
            .filter(|path| path.is_dir())
            .collect::<BTreeSet<_>>();
        for (index, directory) in social_layout_dirs.iter().enumerate() {
            sources.push(SourceMount::new(
                directory.clone(),
                format!("plugins/social/{index}"),
            ));
        }

        // Add configuration file last, or we might run into overlapping paths.
        // Note that right now, we need to monitor the whole directory. We'll
        // integrate identification generation deeper into the file agent,
        // so we can make sure that there won't be any ambiguities.
        let mut path = config.path.clone();
        path.pop();
        sources.push(SourceMount::fallback(
            config.output_root().as_path().to_owned(),
        ));
        for (i, root) in config.api.roots.iter().enumerate() {
            sources.push(SourceMount::new(
                if root.is_file() {
                    root.parent().unwrap().to_owned()
                } else {
                    root.clone()
                },
                format!("api/{i}"),
            ));
        }
        sources.push(SourceMount::fallback(path));

        // Original paths are tracked so discovery of a symlink and its target
        // is not mistaken for a change to an already discovered file.
        let mut seen = BTreeSet::new();

        // Normalize watched paths once, so path comparisons stay stable across
        // platforms and watcher backends (notably on Windows).
        let config_path = canonical_or_clone(&config.path);
        let docs_path = canonical_or_clone(config.docs_root().as_path());
        let theme_dirs = config
            .theme_dirs
            .iter()
            .map(|path| canonical_or_clone(path))
            .collect::<Vec<_>>();
        let watched_files = config
            .project
            .watched_files
            .iter()
            .map(|(path, _)| canonical_or_clone(path))
            .collect::<BTreeSet<_>>();
        // Initialize file agent - we use a debounce interval of 20ms, which
        // should be sufficient to correctly determine rename events
        let manager = Manager::new()
            .include_hidden(config.docs_root().as_path().to_owned());
        let handler = {
            let config = config.clone();
            let mut discovered = false;
            move |results| {
                let mut batch = Vec::new();
                let mut sources_ready = false;
                for res in results {
                    // For now, we just swallow errors from the file agent.
                    let Ok(event) = res else {
                        continue;
                    };

                    // The root creation event is delivered with its complete
                    // recursive scan, including when no documents are found.
                    if !discovered
                        && matches!(&event, Event::Create { kind: Kind::Folder, path }
                            if canonical_or_clone(path) == docs_path)
                    {
                        discovered = true;
                        sources_ready = true;
                    }

                    // Skip anything other than files and symbolic links.
                    // Link events allow assets provided via editable installs
                    // (e.g. symlinked theme directories) to enter the build.
                    if !matches!(event.kind(), Kind::File | Kind::Link) {
                        continue;
                    }

                    // Ignore symbolic links that don't resolve to files.
                    // Directory links are expanded by the watcher backend,
                    // but forwarding the link itself would later be treated as
                    // a file in the workflow and can fail with IsADirectory.
                    if event.kind() == Kind::Link
                        && !fs::metadata(event.path().as_path())
                            .is_ok_and(|meta| meta.is_file())
                    {
                        continue;
                    }

                    // Canonicalize once to compare against configured paths,
                    // which avoids mismatches between equivalent path forms.
                    let event_path = canonical_or_clone(&event.path());

                    if config.api.changed(&event_path)
                        || matches!(&event, Event::Rename { from, .. } if config.api.changed(from))
                    {
                        return Err(Error::Disconnected);
                    }
                    // API inputs only trigger rediscovery; they are not site assets.
                    if config.api.is_source(&event_path)
                        && !event_path.starts_with(config.docs_root().as_path())
                    {
                        continue;
                    }
                    if should_restart(
                        &event,
                        &event_path,
                        &config_path,
                        &theme_dirs,
                        &watched_files,
                        &mut seen,
                    ) {
                        return Err(Error::Disconnected);
                    }

                    // Ignore events in the site directory, since they are files
                    // that were generated and should not trigger a rebuild. We
                    // forward them to the reload channel in the server instead,
                    // so the browser can refresh the site.
                    let site_dir = config.output_root().as_path();
                    if event_path.starts_with(site_dir) {
                        // Compute identifier, since we need the relative URL
                        // so we only reload the page the client is on.
                        let event_path = event.path();
                        let id = to_id(&event_path, &sources)?;

                        // Compute path, and if directory URLs are enabled,
                        // strip the `index.html` suffix, if present.
                        let path = id.as_uri().to_string();
                        let path = if config.project.use_directory_urls {
                            path.trim_end_matches("index.html")
                        } else {
                            path.as_str()
                        };

                        // Prepend base path
                        let base = config.get_base_path();
                        let path = if base == "/" {
                            format!("{base}{path}")
                        } else {
                            format!("{base}/{path}")
                        };

                        // Send path to reload channel and wake server polling
                        // loop, if available (i.e., serve mode is enabled)
                        let _ = reload.send(path);
                        if let Some(waker) = &waker {
                            waker.wake()?;
                        }

                        // We don't trigger rebuilds for the site directory
                        continue;
                    }

                    // Rebuilds can be triggered by external watched dependencies.
                    // Only paths with a source mount are admitted to the workflow.
                    let mounted = |path: &Path| {
                        sources
                            .iter()
                            .any(|source| path.starts_with(&source.root))
                    };
                    match event {
                        // File was created or modified
                        Event::Create { path, .. }
                        | Event::Modify { path, .. }
                            if mounted(&path) =>
                        {
                            for id in to_ids(&path, &sources)? {
                                batch.push(Change::Insert(
                                    id.into(),
                                    Source::from(path.clone()),
                                ));
                            }
                        }

                        // File was renamed
                        Event::Rename { from, to, .. } => {
                            if mounted(&from) {
                                for id in to_ids(&from, &sources)? {
                                    batch.push(Change::Remove(id.into()));
                                }
                            }
                            if mounted(&to) {
                                for id in to_ids(&to, &sources)? {
                                    batch.push(Change::Insert(
                                        id.into(),
                                        Source::from(to.clone()),
                                    ));
                                }
                            }
                        }

                        // File was removed
                        Event::Remove { path, .. } if mounted(&path) => {
                            for id in to_ids(&path, &sources)? {
                                batch.push(Change::Remove(id.into()));
                            }
                        }
                        _ => {}
                    }
                }

                // Apply ownership to both sides of renames, then add the
                // initial generated sources in this same settled revision.
                batch.retain(|change| {
                    let key = match change {
                        Change::Insert(key, _) | Change::Remove(key) => key,
                    };
                    key[0].context() != config.project.docs_dir
                        || key[0].location().parse::<SourcePath>().is_ok_and(
                            |path| !config.api.files.contains_key(&path),
                        )
                });
                batch.append(&mut generated);
                if sources_ready || !batch.is_empty() {
                    changes.send(Batch { changes: batch, sources_ready })?;
                }
                Ok(())
            }
        };
        let agent = Agent::with_manager(
            Duration::from_millis(20),
            serve,
            manager,
            handler,
        );

        // Watch docs and template directories
        agent.watch(&config.path)?;
        for theme_dir in &config.theme_dirs {
            // Skip `.icons` directory. On NetBSD, kqueue opens one file
            // descriptor per file/directory, quickly reaching limits set by
            // the system on the number of open file descriptors.
            match fs::read_dir(theme_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.file_name() != Some(OsStr::new(".icons")) {
                            agent.watch(&path)?;
                        }
                    }
                }
                Err(_) => {
                    // Fall back to watching the whole theme directory if we
                    // cannot enumerate its contents for any reason.
                    agent.watch(theme_dir)?;
                }
            }
        }

        // Custom social card layouts are runtime inputs, just like Markdown
        // and assets. Forward their changes into the retained workflow so
        // cards and metadata can be derived again without restarting serve.
        for directory in social_layout_dirs {
            agent.watch(directory)?;
        }

        // Watch files used by extensions
        for root in &config.api.roots {
            agent.watch(root)?;
        }
        for (path, _) in &config.project.watched_files {
            agent.watch(path)?;
        }

        // Watch site directory, ensuring it exists
        let site_dir = config.output_root().as_path();
        fs::create_dir_all(site_dir).unwrap();
        agent.watch(site_dir)?;

        // Return file watcher
        agent.watch(config.docs_root().as_path())?;
        Ok(Self {
            _agent: agent,
            changes: receiver,
        })
    }

    /// Receives the next debounced source-change batch.
    pub fn receive(
        &self, timeout: Duration,
    ) -> std::result::Result<Batch, RecvTimeoutError> {
        self.changes.recv_timeout(timeout)
    }
}

impl SourceMount {
    /// Creates a source mount with platform-independent context spelling.
    fn new(root: PathBuf, context: String) -> Self {
        Self {
            root,
            context: context.replace('\\', "/"),
            fallback: false,
        }
    }

    /// A fallback mount is created for paths outside logical source roots.
    fn fallback(root: PathBuf) -> Self {
        Self {
            root,
            context: String::from("."),
            fallback: true,
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Create identifier for the given path and sources.
///
/// This will also be hoisted into the file provider, which will make sure that
/// identifiers are platform independent by always ensuring forward slashes.
fn to_id(path: &Path, sources: &[SourceMount]) -> io::Result<Id> {
    let option = sources.iter().find_map(|source| {
        path.strip_prefix(&source.root)
            .ok()
            .map(|suffix| (source, suffix))
    });

    // Note that this cannot fail, since there must be a path in the source
    // mapping that matches the given path, at least the project root
    let (source, suffix) = option.expect("invariant");
    let location = SourcePath::from_path(suffix).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid source path '{}': {error}", path.display()),
        )
    })?;
    Ok(Id::builder()
        .provider("file")
        .context(&source.context)
        .location(location.as_str())
        .build()
        .expect("invariant"))
}

/// All logical identities are returned for overlapping source roots.
fn to_ids(path: &Path, sources: &[SourceMount]) -> io::Result<Vec<Id>> {
    // Physical discovery is deduplicated by the file manager. Each logical
    // mount must still be registered so nested themes retain both outputs.
    let mut ids = sources
        .iter()
        .filter(|source| !source.fallback && path.starts_with(&source.root))
        .map(|source| to_id(path, std::slice::from_ref(source)))
        .collect::<io::Result<Vec<_>>>()?;

    // The project mount is used only when no logical source root is matched.
    if ids.is_empty() {
        ids.push(to_id(path, sources)?);
    }
    Ok(ids)
}

#[inline]
fn canonical_or_clone(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A restart is requested when a previously discovered input is seen again.
fn should_restart(
    event: &Event, event_path: &Path, config_path: &Path,
    theme_dirs: &[PathBuf], watched_files: &BTreeSet<PathBuf>,
    seen: &mut BTreeSet<Arc<PathBuf>>,
) -> bool {
    // Discovery is recorded once per event because restart categories can overlap.
    let watched = event_path == config_path
        || theme_dirs.iter().any(|dir| event_path.starts_with(dir))
        || watched_files.contains(event_path);
    watched && !seen.insert(event.path())
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::{fs, io};
    use tempfile::tempdir;

    use super::{
        should_restart, to_id, to_ids, Event, Kind, Manager, SourceMount,
    };

    #[test]
    fn restart_inputs_are_discovered_once_and_restarted_after_edits() {
        let config = PathBuf::from("project/zensical.toml");
        let theme = PathBuf::from("project/docs/.overrides");
        let icon = theme.join(".icons/custom.svg");
        let template = theme.join("main.html");
        let include = PathBuf::from("project/docs/fragment.j2");

        // Inputs are covered by individual and overlapping restart categories.
        let cases = [
            (config.clone(), vec![], BTreeSet::new()),
            (template, vec![theme.clone()], BTreeSet::new()),
            (include.clone(), vec![], BTreeSet::from([include])),
            (config.clone(), vec![], BTreeSet::from([config.clone()])),
            (
                icon.clone(),
                vec![theme.clone()],
                BTreeSet::from([icon.clone()]),
            ),
            (
                icon.clone(),
                vec![theme.clone(), theme.join(".icons")],
                BTreeSet::new(),
            ),
        ];

        for (path, theme_dirs, watched_files) in cases {
            let mut seen = BTreeSet::new();
            let discovery = Event::Create {
                kind: Kind::File,
                path: Arc::new(path.clone()),
            };

            // Initial discovery must be admitted even if categories overlap.
            assert!(
                !should_restart(
                    &discovery,
                    &path,
                    &config,
                    &theme_dirs,
                    &watched_files,
                    &mut seen,
                ),
                "initial discovery of {path:?}"
            );

            let edit = Event::Modify {
                kind: Kind::File,
                path: Arc::new(path.clone()),
            };

            // A restart must still be requested by a subsequent edit.
            assert!(
                should_restart(
                    &edit,
                    &path,
                    &config,
                    &theme_dirs,
                    &watched_files,
                    &mut seen,
                ),
                "edit to {path:?}"
            );
        }
    }

    #[test]
    fn ordinary_document_edits_do_not_restart_the_build() {
        let path = PathBuf::from("project/docs/index.md");
        let event = Event::Modify {
            kind: Kind::File,
            path: Arc::new(path.clone()),
        };
        let config = PathBuf::from("project/zensical.toml");
        let mut seen = BTreeSet::new();

        // Ordinary document events are admitted to the existing workflow.
        for _ in 0..2 {
            assert!(!should_restart(
                &event,
                &path,
                &config,
                &[],
                &BTreeSet::new(),
                &mut seen,
            ));
        }
    }

    #[test]
    fn watched_symlink_aliases_are_discovered_independently() {
        let config = PathBuf::from("project/zensical.toml");
        let target = PathBuf::from("project/fragment.j2");
        let watched_files = BTreeSet::from([target.clone()]);
        let mut seen = BTreeSet::new();

        // Aliases are resolved to the same watched target during discovery.
        for path in ["project/docs/first.j2", "project/docs/second.j2"] {
            let discovery = Event::Create {
                kind: Kind::Link,
                path: Arc::new(PathBuf::from(path)),
            };

            assert!(!should_restart(
                &discovery,
                &target,
                &config,
                &[],
                &watched_files,
                &mut seen,
            ));
        }

        let edit = Event::Modify {
            kind: Kind::Link,
            path: Arc::new(PathBuf::from("project/docs/first.j2")),
        };

        // A restart must be requested when an existing alias is seen again.
        assert!(should_restart(
            &edit,
            &target,
            &config,
            &[],
            &watched_files,
            &mut seen,
        ));
    }

    #[test]
    fn hidden_directory_discovery_is_limited_to_documentation() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("project");
        let docs = root.join("docs");
        let page = docs.join(".private/page.md");
        let icon = root.join("theme/.icons/icon.svg");
        for file in [&page, &icon] {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "content").unwrap();
        }

        // Hidden docs files are discovered while theme icons remain skipped.
        let mut manager = Manager::new().include_hidden(docs);
        let paths = manager
            .handle([root])
            .into_iter()
            .map(|event| event.unwrap().path())
            .collect::<Vec<_>>();

        assert!(paths.iter().any(|path| path.as_ref() == &page));
        assert!(!paths.iter().any(|path| path.as_ref() == &icon));
    }

    #[test]
    fn output_identifiers_do_not_contain_the_physical_root() {
        let directory = tempdir().unwrap();
        let output = directory.path().join("absolute/site");
        fs::create_dir_all(&output).unwrap();
        let file = output.join("guide/index.html");
        let sources = [SourceMount::new(output, String::from("."))];

        let id = to_id(&file, &sources).unwrap();

        assert_eq!(id.context(), ".");
        assert_eq!(id.location(), "guide/index.html");
        assert_eq!(id.as_uri().as_str(), "guide/index.html");
    }

    #[test]
    fn external_plugin_sources_use_their_own_mount() {
        let directory = tempdir().unwrap();
        let layouts = directory.path().join("shared/layouts");
        fs::create_dir_all(&layouts).unwrap();
        let file = layouts.join("custom.yml");
        let sources =
            [SourceMount::new(layouts, String::from("plugins/social/0"))];

        let id = to_id(&file, &sources).unwrap();

        assert_eq!(id.context(), "plugins/social/0");
        assert_eq!(id.location(), "custom.yml");
    }

    #[test]
    fn overlapping_mounts_retain_each_logical_identity() {
        let root = PathBuf::from("project");
        let docs = root.join("docs");
        let theme = docs.join("overrides");
        let path = theme.join("assets/probe.css");
        let sources = [
            SourceMount::new(docs, String::from("docs")),
            SourceMount::new(theme.clone(), String::from("templates/0")),
            SourceMount::new(
                theme.join("assets"),
                String::from("plugins/social/0"),
            ),
            SourceMount::fallback(root),
        ];

        // No filesystem lookup is required, so removals retain all identities.
        let ids = to_ids(&path, &sources).unwrap();
        let identities = ids
            .iter()
            .map(|id| (id.context().into_owned(), id.location().to_string()))
            .collect::<Vec<_>>();

        assert_eq!(
            identities,
            vec![
                (
                    String::from("docs"),
                    String::from("overrides/assets/probe.css")
                ),
                (
                    String::from("templates/0"),
                    String::from("assets/probe.css")
                ),
                (String::from("plugins/social/0"), String::from("probe.css")),
            ]
        );
        let fallback =
            to_ids(&PathBuf::from("project/config.yml"), &sources).unwrap();
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].context(), ".");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_provider_identity_instead_of_collapsing_it() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;

        let directory = tempdir().unwrap();
        let file = directory
            .path()
            .join(OsString::from_vec(b"bad-\xff.md".to_vec()));
        let sources = [SourceMount::new(
            directory.path().to_owned(),
            String::from("docs"),
        )];

        let error = to_id(&file, &sources).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
