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

use crossbeam::channel::unbounded;
use pyo3::create_exception;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::PyTypeInfo;
use pyo3::Python;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant};
use std::{fs, io, thread};
use zrx::scheduler::action::Report;
use zrx::scheduler::Scheduler;
use zrx_diagnostic::Severity;

mod config;
mod server;
mod structure;
mod template;
mod watcher;
mod workflow;

use config::Config;
use server::{create_server, ServeOptions};
use watcher::Watcher;
use workflow::create_workspace;

create_exception!(
    zensical,
    StrictModeError,
    PyRuntimeError,
    "Raised when ``--strict`` is set and the build produced warnings or error-level diagnostics."
);

const STRICT_MODE_ERROR_MSG: &str =
    "Build failed: strict mode treats warnings as errors.";

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Build mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Build the project once.
    Build(bool),
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

// When displaying diagnostics, we want to keep track if any are a violation of strict mode.
fn handle_report(report: Report) -> bool {
    let mut strict_mode_violated = false;
    for diagnostic in &report {
        if matches!(diagnostic.severity, Severity::Warning | Severity::Error) {
            strict_mode_violated = true;
        }
        println!("[{:?}] {}", diagnostic.severity, diagnostic.message);
    }
    strict_mode_violated
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

/// Run the build process.
fn run(config_file: &PathBuf, mode: Mode, strict: bool) -> PyResult<bool> {
    #[cfg(feature = "tracing")]
    let _guard = setup_tracing();

    // In case the configuration changes, we recreate the entire workspace and
    // scheduler. Once we have the module system set up, this will be tightly
    // integrated and not necessary anymore, since partial rebuilds of the
    // network of tasks will be supported.
    let config = match Config::new(config_file) {
        Ok(config) => config,
        // If we're in serve mode, we can just wait until the configuration
        // file is touched and start over again
        Err(err) if matches!(mode, Mode::Serve(_, _)) => {
            println!("[error] Failed to load configuration: {err}");
            return wait_for_touch(config_file).map_err(Into::into);
        }
        Err(err) => return Err(err.into()),
    };

    // Clean cache directory if requested
    if let Mode::Build(true) = mode {
        let cache_dir = config.get_cache_dir();
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)
                .expect("cache directory could not be removed");
        }
    }

    // Always clean site directory before building for now - we're working on
    // true differential builds, which will also include cleaning up old files
    // that are not needed anymore but for now, we just remove everything, like
    // MkDocs does it.
    let site_dir = config.get_site_dir();
    if site_dir.exists() {
        std::fs::remove_dir_all(&site_dir)
            .expect("site directory could not be removed");
    }

    // Create workspace and scheduler
    let workspace = create_workspace(&config);
    let mut scheduler = Scheduler::new(workspace.into_builder().build());

    // Create channel for reload notifications
    let (sender, receiver) = unbounded();

    // Create session to connect file agent and scheduler - note that we must
    // assign the agent to a variable right now, or it is dropped, and will
    // automatically terminate. This is a temporary workaround until we could
    // better integrate the scheduler with the agent.
    let session = scheduler.session().expect("invariant");

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
    let watcher = Watcher::new(&config, session, sender, waker.clone())?;

    // Hack: the scheduler and file agent are currently not synchronized, which
    // can lead to cases where the file agent is still busy reading the contents
    // of the docs directory before starting to emit anything, and the scheduler
    // starting off while having nothing to do. We need to improve communication
    // between both parts of the system. In the meantime, we wait until the
    // scheduler has something to do, before kicking off work.
    while scheduler.is_empty() {
        thread::sleep(Duration::from_millis(10));
    }

    // Start event loop after a short delay - once we tightly integrated the
    // file agent with the scheduler, the sleep can be removed
    println!("Build started");
    let time = Instant::now();
    let mut strict_mode_violated = false;
    loop {
        match &mode {
            // Build mode - just exit when we're done
            Mode::Build(..) => {
                strict_mode_violated |= handle_report(
                    scheduler.tick_timeout(Duration::from_millis(100)),
                );
                if scheduler.is_empty() {
                    let elapsed = time.elapsed().as_secs_f32();
                    println!("Build finished in {elapsed:.2}s");
                    break;
                }
            }
            // Serve mode - keep watching, until the watcher terminates, which
            // happens if the configuration file changed. After we've integrated
            // the scheduler with the agent, we can remove this temporary hack
            // and have immediate reloading.
            Mode::Serve(..) => {
                strict_mode_violated |= handle_report(
                    scheduler.tick_timeout(Duration::from_millis(100)),
                );
                if strict && strict_mode_violated {
                    return Err(StrictModeError::new_err(
                        STRICT_MODE_ERROR_MSG,
                    ));
                }
                if watcher.is_terminated() {
                    // Wake the server
                    if let Some(waker) = &waker {
                        waker.wake()?;
                    }
                    return Ok(true);
                }
            }
        }

        // Allow Python to handle signals (e.g., Ctrl+C)
        if Python::attach(|py| py.check_signals().is_err()) {
            println!("Received interrupt, exiting");
            break;
        }
    }

    if strict && strict_mode_violated {
        return Err(StrictModeError::new_err(STRICT_MODE_ERROR_MSG));
    }

    // All good
    Ok(false)
}

// ----------------------------------------------------------------------------

/// Builds the project.
#[pyfunction]
fn build(
    py: Python, config_file: PathBuf, clean: bool, strict: bool,
) -> PyResult<()> {
    py.detach(|| {
        run(&config_file, Mode::Build(clean), strict)?;
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
        let strict = options.strict;
        match run(&config_file, Mode::Serve(options.clone(), seq), strict) {
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

#[cfg(test)]
mod tests {
    use super::handle_report;
    use zrx::scheduler::action::Report;
    use zrx_diagnostic::{Diagnostic, Severity};

    #[test]
    fn handle_report_returns_false_without_warnings() {
        let report =
            Report::new(()).with([Diagnostic::new(Severity::Info, "ok")]);
        assert!(!handle_report(report));
    }

    #[test]
    fn handle_report_returns_true_when_warning_present() {
        let report = Report::new(())
            .with([Diagnostic::new(Severity::Warning, "heads up")]);
        assert!(handle_report(report));
    }

    #[test]
    fn handle_report_returns_true_when_error_present() {
        let report =
            Report::new(()).with([Diagnostic::new(Severity::Error, "failed")]);
        assert!(handle_report(report));
    }
}

// ----------------------------------------------------------------------------

/// Expose Rust runtime to Python.
#[pymodule]
fn zensical(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build, m)?)?;
    m.add_function(wrap_pyfunction!(serve, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add("StrictModeError", StrictModeError::type_object(m.py()))?;
    Ok(())
}
