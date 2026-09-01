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

//! Zensical Python bindings.

#![allow(clippy::default_constructed_unit_structs)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::needless_pass_by_value)]

use crossbeam::channel::{unbounded, RecvTimeoutError};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::Python;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fs, io, thread};
use zrx::id::Id;
use zrx::stream::{Change, Key};

mod compat;
mod config;
mod python;
mod server;
mod structure;
mod template;
mod watcher;
mod workflow;

use compat::mkdocs::plugin::meta;
use config::Config;
use server::{create_server, ServeOptions};
use watcher::{Source, Watcher};
use workflow::{create_workflow, Input};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Serve options.
#[derive(Clone, Debug, FromPyObject, PartialEq, Eq)]
#[pyo3(from_item_all)]
pub struct BuildOptions {
    /// Whether to clean the cache directory before building.
    pub clean: Option<bool>,
    /// Whether to enable strict mode - abort the build on any warnings.
    pub strict: Option<bool>,
}

/// Build mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Build the project once.
    Build(BuildOptions),
    /// Build the project continuously.
    Serve(ServeOptions, u64),
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Setup tracing if enabled.
#[cfg(feature = "tracing")]
fn setup_tracing() -> tracing_chrome::FlushGuard {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;
    let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file("trace.json")
        .include_args(true)
        .include_locations(true)
        .build();

    // Create and subscribe tracing subscriber
    let subscriber = Registry::default().with(chrome_layer);
    let _ = tracing::subscriber::set_global_default(subscriber);
    guard
}

/// Wait until the file at the given path is touched.
///
/// During the wait we also poll for Python signal handling so a keyboard
/// interrupt (Ctrl‑C) can abort the blocking loop.
fn wait_for_touch(path: &Path) -> io::Result<bool> {
    let last = fs::metadata(path)?.modified()?;
    loop {
        thread::sleep(Duration::from_millis(250));
        if last < fs::metadata(path)?.modified()? {
            break;
        }

        // Allow Python to handle signals (e.g., Ctrl+C)
        if Python::attach(|py| py.check_signals().is_err()) {
            println!("Received interrupt, exiting");
            process::exit(1);
        }
    }
    Ok(true)
}

/// Clears the contents of a directory without removing the directory itself.
fn clear_dir(dir: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();

        // Only remove non-hidden paths (not starting with `.`) to match
        // MkDocs' behavior. This allows users to track the (empty) site folder
        // by adding a `.gitkeep` file within it.
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
            if path.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

/// Run the build process.
#[allow(clippy::too_many_lines)]
fn run(config_file: &PathBuf, mode: Mode) -> PyResult<bool> {
    #[cfg(feature = "tracing")]
    let _guard = setup_tracing();

    // In case the configuration changes, we recreate the entire workspace and
    // scheduler. Once we have the module system set up, this will be tightly
    // integrated and not necessary anymore, since partial rebuilds of the
    // network of tasks will be supported.
    let config = match Config::new(config_file) {
        Ok(config) => config,
        // If we're already serving (seq > 0), a previous build succeeded, so
        // we can wait for the config file to be fixed and retry. On the first
        // run (seq == 0) we exit immediately, just like `build` does.
        Err(err) if matches!(&mode, Mode::Serve(_, seq) if *seq > 0) => {
            println!("[error] Failed to load configuration: {err}");
            return wait_for_touch(config_file).map_err(Into::into);
        }
        Err(err) => return Err(err.into()),
    };

    // Clean cache directory if requested
    if let Mode::Build(options) = &mode {
        if options.clean.unwrap_or(false) {
            let cache_dir = config.get_cache_dir();
            if cache_dir.exists() {
                std::fs::remove_dir_all(&cache_dir)
                    .expect("cache directory could not be removed");
            }
        }
    }

    // Always clean site directory before building for now - we're working on
    // true differential builds, which will also include cleaning up old files
    // that are not needed anymore but for now, we just remove everything, like
    // MkDocs does it, but not the directory itself, see https://t.ly/Lrjdx
    let site_dir = config.get_site_dir();
    if site_dir.exists() {
        clear_dir(&site_dir).expect("site directory could not be cleaned");
    }

    // Determine if strict mode is enabled
    let strict = match &mode {
        Mode::Build(options) => {
            options.strict.unwrap_or(false) || config.project.strict
        }
        Mode::Serve(_, _) => false,
    };

    // Create workflow runner and acquire its source input
    let workflow = create_workflow(&config, strict);
    let mut runner = workflow
        .runner()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let mut input = runner
        .input::<Input>()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let meta_settings = meta::Settings::new(&config);

    // Create channel for reload notifications
    let (sender, receiver) = unbounded();

    // If site should be served, create HTTP server - note that we must assign
    // the agent to a variable right now or it's dropped and will automatically
    // terminate. This is a temporary workaround until we could better integrate
    // the scheduler with the agent.
    let waker = match &mode {
        Mode::Build(_) => None,
        Mode::Serve(options, seq) => {
            if *seq == 0 {
                println!(
                    "Serving {} on http://{}",
                    site_dir.display(),
                    options
                        .dev_addr
                        .as_ref()
                        .unwrap_or_else(|| &config.project.dev_addr)
                );
            } else {
                println!("Reloading...");
            }
            Some(create_server(&config, receiver, options.clone()))
        }
    };

    let serve = matches!(mode, Mode::Serve(_, _));
    let watcher = Watcher::new(&config, serve, sender, waker.clone())?;

    // Start the event loop. Each debounced watcher batch is admitted as one
    // source revision and fully settled before the next batch is accepted.
    println!("Build started");
    let time = Instant::now();
    loop {
        match watcher.receive(Duration::from_millis(100)) {
            Ok(changes) => {
                let metadata = Arc::new(
                    meta::Index::load(&config.get_docs_dir(), &meta_settings)
                        .map_err(|error| {
                        PyRuntimeError::new_err(format!("{error:#}"))
                    })?,
                );
                let dependents = metadata_dependents(
                    &changes,
                    &config.get_docs_dir(),
                    &meta_settings,
                )?;
                let mut revision = input
                    .begin()
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                for (key, source) in dependents {
                    revision
                        .insert(key, Input::new(source, metadata.clone()))
                        .map_err(|err| {
                            PyRuntimeError::new_err(err.to_string())
                        })?;
                }
                for change in changes {
                    match change {
                        Change::Insert(key, source) => revision
                            .insert(key, Input::new(source, metadata.clone()))
                            .map_err(|err| {
                                PyRuntimeError::new_err(err.to_string())
                            })?,
                        Change::Remove(key) => {
                            revision.remove(key).map_err(|err| {
                                PyRuntimeError::new_err(err.to_string())
                            })?;
                        }
                    }
                }
                input = revision
                    .seal()
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;

                let run = runner
                    .settle()
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                report_failures(&run)?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => match mode {
                Mode::Build(..) => {
                    let elapsed = time.elapsed().as_secs_f32();
                    println!("Build finished in {elapsed:.2}s");
                    break;
                }
                Mode::Serve(..) => {
                    // Wake the server
                    if let Some(waker) = &waker {
                        waker.wake()?;
                    }
                    return Ok(true);
                }
            },
        }

        // Allow Python to handle signals (e.g., Ctrl+C)
        if Python::attach(|py| py.check_signals().is_err()) {
            println!("Received interrupt, exiting");
            std::process::exit(0);
        }
    }

    // All good
    Ok(false)
}

/// Expands metadata-file changes into descendant Markdown updates.
fn metadata_dependents(
    changes: &[Change<Id, Source>], docs: &Path, settings: &meta::Settings,
) -> PyResult<Vec<(Key<Id>, Source)>> {
    if !settings.enabled {
        return Ok(Vec::new());
    }
    let mut dependents = BTreeMap::new();
    for change in changes {
        let key = match change {
            Change::Insert(key, _) | Change::Remove(key) => key,
        };
        let location = key[0].location();
        if !meta::claims(&location, settings) {
            continue;
        }
        let parent = Path::new(location.as_ref())
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let mut paths = Vec::new();
        collect_markdown(&docs.join(parent), &mut paths)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        for path in paths {
            let relative = path
                .strip_prefix(docs)
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
            let location = relative.to_string_lossy().replace('\\', "/");
            let id = key[0]
                .to_builder()
                .location(location)
                .build()
                .expect("invariant");
            dependents.insert(
                Key::from(id),
                Source::from(path.to_string_lossy().into_owned()),
            );
        }
    }

    // A provider update for the page itself is authoritative. In particular,
    // the initial snapshot contains both metadata files and every Markdown
    // page, so retaining synthesized inserts here would admit each page twice.
    for change in changes {
        let key = match change {
            Change::Insert(key, _) | Change::Remove(key) => key,
        };
        dependents.remove(key);
    }
    Ok(dependents.into_iter().collect())
}

/// Recursively collects Markdown files below one metadata directory.
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

/// Returns the first action failure reported by one settled run.
fn report_failures(run: &zrx::stream::Run<Id>) -> PyResult<()> {
    for invocation in run.report().invocations() {
        if let Some(failure) = invocation.outcomes.failures().first() {
            return Err(PyRuntimeError::new_err(format!("{failure:#}")));
        }
    }
    Ok(())
}

// ----------------------------------------------------------------------------

/// Builds the project.
#[pyfunction]
fn build(
    py: Python, config_file: PathBuf, options: BuildOptions,
) -> PyResult<()> {
    py.detach(|| {
        run(&config_file, Mode::Build(options))?;
        Ok(())
    })
}

/// Builds and serves the project.
#[pyfunction]
fn serve(
    py: Python, config_file: PathBuf, mut options: ServeOptions,
) -> PyResult<()> {
    let mut seq = 0;
    py.detach(|| loop {
        match run(&config_file, Mode::Serve(options.clone(), seq)) {
            Ok(true) => {
                options.open = false;
                seq += 1;
            }
            other => return other.map(|_| ()),
        }
    })
}

/// Returns the current version.
#[pyfunction]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ----------------------------------------------------------------------------

/// Expose Rust runtime to Python.
#[pymodule]
fn zensical(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build, m)?)?;
    m.add_function(wrap_pyfunction!(serve, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn clear_dir_removes_non_hidden_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");

        fs::write(&file, "hello").unwrap();

        clear_dir(dir.path()).unwrap();

        assert!(!file.exists());
        assert!(dir.path().exists());
    }

    #[test]
    fn clear_dir_removes_non_hidden_directory_recursively() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("subdir");

        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("nested.txt"), "hello").unwrap();

        clear_dir(dir.path()).unwrap();

        assert!(!subdir.exists());
        assert!(dir.path().exists());
    }

    #[test]
    fn clear_dir_preserves_hidden_file() {
        let dir = tempdir().unwrap();
        let hidden = dir.path().join(".gitkeep");

        fs::write(&hidden, "").unwrap();

        clear_dir(dir.path()).unwrap();

        assert!(hidden.exists());
    }

    #[test]
    fn clear_dir_preserves_hidden_directory() {
        let dir = tempdir().unwrap();
        let hidden_dir = dir.path().join(".cache");

        fs::create_dir(&hidden_dir).unwrap();
        fs::write(hidden_dir.join("file.txt"), "hello").unwrap();

        clear_dir(dir.path()).unwrap();

        assert!(hidden_dir.exists());
        assert!(hidden_dir.join("file.txt").exists());
    }

    #[test]
    fn clear_dir_removes_only_non_hidden_entries() {
        let dir = tempdir().unwrap();

        let file = dir.path().join("file.txt");
        let subdir = dir.path().join("subdir");
        let hidden_file = dir.path().join(".gitkeep");
        let hidden_dir = dir.path().join(".cache");

        fs::write(&file, "hello").unwrap();

        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("nested.txt"), "hello").unwrap();

        fs::write(&hidden_file, "").unwrap();

        fs::create_dir(&hidden_dir).unwrap();
        fs::write(hidden_dir.join("nested.txt"), "hello").unwrap();

        clear_dir(dir.path()).unwrap();

        assert!(!file.exists());
        assert!(!subdir.exists());

        assert!(hidden_file.exists());
        assert!(hidden_dir.exists());
        assert!(hidden_dir.join("nested.txt").exists());

        assert!(dir.path().exists());
    }

    #[test]
    fn clear_dir_empty_directory_is_ok() {
        let dir = tempdir().unwrap();

        clear_dir(dir.path()).unwrap();

        assert!(dir.path().exists());
    }

    #[test]
    fn metadata_change_selects_only_descendant_markdown() {
        let dir = tempdir().unwrap();
        let docs = dir.path();
        fs::create_dir_all(docs.join("guide/nested")).unwrap();
        fs::create_dir_all(docs.join("guidelines")).unwrap();
        fs::write(docs.join("guide/page.md"), "# Page").unwrap();
        fs::write(docs.join("guide/nested/page.md"), "# Nested").unwrap();
        fs::write(docs.join("guidelines/page.md"), "# Other").unwrap();

        let id = Id::builder()
            .provider("file")
            .context("docs")
            .location("guide/.meta.yml")
            .build()
            .unwrap();
        let changes = vec![Change::Insert(
            Key::from(id),
            Source::from(docs.join("guide/.meta.yml").display().to_string()),
        )];
        let settings = meta::Settings {
            enabled: true,
            meta_file: ".meta.yml".into(),
        };
        let dependents =
            metadata_dependents(&changes, docs, &settings).unwrap();
        let locations = dependents
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

        let meta_id = Id::builder()
            .provider("file")
            .context("docs")
            .location("guide/.meta.yml")
            .build()
            .unwrap();
        let page_id = Id::builder()
            .provider("file")
            .context("docs")
            .location("guide/page.md")
            .build()
            .unwrap();
        let changes = vec![
            Change::Insert(
                Key::from(meta_id),
                Source::from(
                    docs.join("guide/.meta.yml").display().to_string(),
                ),
            ),
            Change::Insert(
                Key::from(page_id),
                Source::from(page.display().to_string()),
            ),
        ];
        let settings = meta::Settings {
            enabled: true,
            meta_file: ".meta.yml".into(),
        };

        assert!(metadata_dependents(&changes, docs, &settings)
            .unwrap()
            .is_empty());
    }
}
