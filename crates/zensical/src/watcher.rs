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
    changes: Receiver<Vec<Change<Id, Source>>>,
}

/// One physical source root and its provider-relative identity context.
struct SourceMount {
    root: PathBuf,
    context: String,
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

        // Add configuration file last, or we might run into overlapping paths.
        // Note that right now, we need to monitor the whole directory. We'll
        // integrate identification generation deeper into the file agent,
        // so we can make sure that there won't be any ambiguities.
        let mut path = config.path.clone();
        path.pop();
        sources.push(SourceMount::new(
            config.output_root().as_path().to_owned(),
            String::from("."),
        ));
        sources.push(SourceMount::new(path, String::from(".")));

        // Track seen files to restart on config or template change
        let mut seen = BTreeSet::new();

        // Normalize watched paths once, so path comparisons stay stable across
        // platforms and watcher backends (notably on Windows).
        let config_path = canonical_or_clone(&config.path);
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
        let agent = Agent::new(Duration::from_millis(20), serve, {
            let config = config.clone();
            move |results| {
                let mut batch = Vec::new();
                for res in results {
                    // For now, we just swallow errors from the file agent.
                    let Ok(event) = res else {
                        continue;
                    };

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

                    // Check if the config file reloaded, and terminate agent,
                    // as we need to kick off the entire pipeline again
                    if event_path == config_path
                        && !seen.insert(config_path.clone())
                    {
                        return Err(Error::Disconnected);
                    }

                    // Check if the event is in any of the theme directories
                    // and restart the build if we've already seen the file
                    for dir in &theme_dirs {
                        if event_path.starts_with(dir)
                            && !seen.insert(event_path.clone())
                        {
                            return Err(Error::Disconnected);
                        }
                    }

                    // Check if one of the source files managed by mkdocstrings
                    // changed, and restart the build
                    if watched_files.contains(&event_path)
                        && !seen.insert(event_path.clone())
                    {
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

                    // Compute an identifier from the path and known contexts.
                    match event {
                        // File was created or modified
                        Event::Create { path, .. }
                        | Event::Modify { path, .. } => {
                            batch.push(Change::Insert(
                                to_id(&path, &sources)?.into(),
                                Source::from(path),
                            ));
                        }

                        // File was renamed
                        Event::Rename { from, to, .. } => {
                            batch.push(Change::Remove(
                                to_id(&from, &sources)?.into(),
                            ));
                            batch.push(Change::Insert(
                                to_id(&to, &sources)?.into(),
                                Source::from(to),
                            ));
                        }

                        // File was removed
                        Event::Remove { path, .. } => {
                            batch.push(Change::Remove(
                                to_id(&path, &sources)?.into(),
                            ));
                        }
                    }
                }

                if !batch.is_empty() {
                    changes.send(batch)?;
                }
                Ok(())
            }
        });

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

        // Watch files used by extensions
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
    ) -> std::result::Result<Vec<Change<Id, Source>>, RecvTimeoutError> {
        self.changes.recv_timeout(timeout)
    }
}

impl SourceMount {
    /// Creates a source mount with platform-independent context spelling.
    fn new(root: PathBuf, context: String) -> Self {
        Self {
            root,
            context: context.replace('\\', "/"),
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

#[inline]
fn canonical_or_clone(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{fs, io};
    use tempfile::tempdir;

    use super::{to_id, SourceMount};

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
