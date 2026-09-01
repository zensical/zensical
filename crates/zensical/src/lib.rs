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
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{
    pyfunction, pymodule, wrap_pyfunction, Bound, FromPyObject, PyResult,
    Python,
};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fs, io, thread};

use zrx::id::Id;
use zrx::stream::{Change, Key};

mod compat;
mod config;
pub mod path;
mod python;
mod server;
mod structure;
mod template;
mod watcher;
mod workflow;

use compat::mkdocs::plugin::meta;
use config::Config;
use server::{create_server, ServeOptions};
use watcher::Watcher;
use workflow::{create_workflow, Configuration, Input};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Build mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Build the project once.
    Build(BuildOptions),
    /// Build the project continuously.
    Serve(ServeOptions, u64),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Build options.
#[derive(Clone, Debug, FromPyObject, PartialEq, Eq)]
#[pyo3(from_item_all)]
pub struct BuildOptions {
    /// Whether to clean the cache directory before building.
    pub clean: Option<bool>,
    /// Whether to enable strict mode and abort on warnings.
    pub strict: Option<bool>,
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
    let site_dir = config.output_root().as_path();
    if site_dir.exists() {
        clear_dir(site_dir).expect("site directory could not be cleaned");
    }

    // Determine if strict mode is enabled
    let strict = match &mode {
        Mode::Build(options) => {
            options.strict.unwrap_or(false) || config.project.strict
        }
        Mode::Serve(_, _) => false,
    };

    // Resolve metadata settings once for the workflow and provider boundary.
    // The provider still needs them while metadata remains a revision fact
    // workaround, so share the module-owned value rather than reprojecting
    // configuration independently on both sides.
    let meta_settings = Arc::new(meta::Settings::new(&config));

    // Create workflow runner and acquire its source input
    let workflow = create_workflow(&config, strict, meta_settings.clone());
    let mut runner = workflow
        .runner()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let mut input = runner
        .input::<Input>()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let configuration = runner
        .input::<Configuration>()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let mut revision = configuration
        .begin()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    revision
        .insert(
            Key::<Id>::from_iter(std::iter::empty()),
            Configuration::new(config.clone(), strict),
        )
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let _configuration = revision
        .seal()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let run = runner
        .settle()
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    report_failures(&run)?;
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
    let mut metadata = meta::Admission::new(
        config.docs_root().clone(),
        config.project.docs_dir.clone(),
        meta_settings,
    );

    // Start the event loop. Each debounced watcher batch is admitted as one
    // source revision and fully settled before the next batch is accepted.
    println!("Build started");
    let time = Instant::now();
    loop {
        match watcher.receive(Duration::from_millis(100)) {
            Ok(changes) => {
                let meta::Prepared { index, dependents } =
                    metadata.prepare(&changes).map_err(|error| {
                        PyRuntimeError::new_err(format!("{error:#}"))
                    })?;
                let mut revision = input
                    .begin()
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                for (key, source) in dependents {
                    revision
                        .insert(key, Input::new(source, index.clone()))
                        .map_err(|err| {
                            PyRuntimeError::new_err(err.to_string())
                        })?;
                }
                for change in changes {
                    match change {
                        Change::Insert(key, source) => revision
                            .insert(key, Input::new(source, index.clone()))
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
    use std::fs;
    use tempfile::tempdir;

    use super::clear_dir;

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
}
