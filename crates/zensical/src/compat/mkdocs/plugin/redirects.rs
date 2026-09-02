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

//! MkDocs-compatible redirects pipeline.

use anyhow::Result;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Stream, Value};

use crate::config::Config;
use crate::path::OutputRoot;
use crate::structure::page::PageRoute;

mod output;
mod plan;

use output::write;
use plan::{Plan, Snapshot};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MkDocs-compatible redirects pipeline.
#[derive(Clone, Debug)]
pub struct Redirects;

// ----------------------------------------------------------------------------

/// Streams consumed by the redirects pipeline.
pub struct Dependencies<'a> {
    /// Module-local settings derived from the shared configuration stream.
    pub settings: &'a Stream<Id, Settings>,
    /// Routes derived before Markdown rendering.
    pub routes: &'a Stream<Id, PageRoute>,
}

// ----------------------------------------------------------------------------

/// Configuration owned by the redirects pipeline.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Configuration work prepared once for the workflow lifetime.
    plan: Arc<Plan>,
    /// Whether page URLs use directories.
    use_directory_urls: bool,
    /// Site output directory.
    output: OutputRoot,
    /// Whether warnings fail the build.
    strict: bool,
}

// ----------------------------------------------------------------------------

/// Compact revision-settled route facts consumed by redirects.
#[derive(Clone, Debug)]
struct Routes(Arc<Vec<PageRoute>>);

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Redirects {
    /// Installs redirect resolution and artifact generation.
    ///
    /// Route updates are first reduced into one revision-settled value. This
    /// prevents the writer from observing a partially updated navigation and
    /// keeps every emitted redirect snapshot internally consistent.
    #[allow(
        clippy::unused_self,
        reason = "setup is the module instance entry point"
    )]
    pub fn setup(&self, dependencies: Dependencies<'_>) {
        let routes = dependencies.routes.reduce(
            |routes: &dyn Collection<zrx::stream::Key<Id>, PageRoute>| {
                Some(Routes(Arc::new(routes.values().cloned().collect())))
            },
        );
        let snapshots = routes.product(dependencies.settings).map(
            |routes: &Routes, settings: &Settings| {
                Ok::<_, anyhow::Error>((
                    Snapshot::new(
                        &settings.plan,
                        routes.0.iter(),
                        settings.use_directory_urls,
                    )?,
                    settings.clone(),
                ))
            },
        );
        let _ = snapshots.map(|snapshot: &(Snapshot, Settings)| {
            write(&snapshot.1.output, &snapshot.0, snapshot.1.strict)
        });
    }
}

// ----------------------------------------------------------------------------

impl Settings {
    /// Extracts and prepares the configuration owned by redirects.
    pub fn new(config: &Config, strict: bool) -> Result<Self> {
        Ok(Self {
            plan: Arc::new(Plan::new(config)?),
            use_directory_urls: config.project.use_directory_urls,
            output: config.output_root().clone(),
            strict,
        })
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Settings {}

// ----------------------------------------------------------------------------

impl Value for Routes {}

// ----------------------------------------------------------------------------

impl Value for Snapshot {}
