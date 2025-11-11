// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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

use crossbeam::channel::Sender;
use mio::Waker;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use zensical_watch::event::{Event, Kind};
use zensical_watch::{Agent, Error, Result};
use zrx::id::Id;
use zrx::scheduler::Session;

use super::config::Config;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// File watcher.
///
/// This is a thin wrapper around the file agent. We're going to refactor this
/// logic into a provider architecture that will make things more flexible.
pub struct Watcher {
    /// File agent.
    agent: Agent,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Watcher {
    /// Creates a file watcher.
    pub fn new(
        config: &Config, session: Session<Id, String>, reload: Sender<String>,
        waker: Option<Arc<Waker>>,
    ) -> Result<Self> {
        let mut sources = Vec::default();

        // Add docs directory and theme directories
        sources.push((config.get_docs_dir(), config.project.docs_dir.clone()));
        for (i, theme_dir) in config.theme_dirs.iter().enumerate() {
            sources.push((theme_dir.clone(), format!("templates/{i}")));
        }

        // Add configuration file last, or we might run into overlapping paths.
        // Note that right now, we need to monitor the whole directory. We'll
        // integrate identification generation deeper into the file agent,
        // so we can make sure that there won't be any ambiguities.
        let mut path = config.path.clone();
        path.pop();
        sources.push((config.get_site_dir(), config.project.site_dir.clone()));
        sources.push((path, String::from(".")));

        // Initialize file agent - we use a debounce interval of 20ms, which
        // should be sufficient to correctly determine rename events
        let mut initial = false;
        let agent = Agent::new(Duration::from_millis(20), {
            let config = config.clone();
            move |res| {
                // For now, we just swallow the event, as the file agent should
                // to take care of it, and skip anything other than files
                if let Ok(event) = res {
                    if event.kind() != Kind::File {
                        return Ok(());
                    }

                    // Check if the config file reloaded, and terminate agent,
                    // as we need to kick off the entire pipeline again
                    if *event.path() == config.path {
                        if initial {
                            return Err(Error::Disconnected);
                        }
                        initial = true;
                    }

                    // Ignore events in the site directory, since they are files
                    // that were generated and should not trigger a rebuild. We
                    // forward them to the reload channel in the server instead,
                    // so the browser can refresh the site.
                    let site_dir = config.get_site_dir();
                    if event.path().starts_with(&site_dir) {
                        // Compute identifier, since we need the relative URL
                        // so we only reload the page the client is on.
                        let id = to_id(event.path().clone(), &sources);

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
                        return Ok(());
                    }

                    // Compute an identifier from the path and known contexts -
                    // in case the session is disconnected, the agent terminates
                    match event {
                        // File was created or modified
                        Event::Create { path, .. }
                        | Event::Modify { path, .. } => {
                            let data = path.to_string_lossy().into_owned();
                            session.insert(to_id(path, &sources), data)?;
                        }

                        // File was renamed
                        Event::Rename { from, to, .. } => {
                            let data = to.to_string_lossy().into_owned();
                            session.remove(to_id(from, &sources))?;
                            session.insert(to_id(to, &sources), data)?;
                        }

                        // File was removed
                        Event::Remove { path, .. } => {
                            session.remove(to_id(path, &sources))?;
                        }
                    }
                }
                Ok(())
            }
        });

        // Watch docs and template directories
        agent.watch(config.get_docs_dir())?;
        agent.watch(&config.path)?;
        for theme_dir in &config.theme_dirs {
            agent.watch(theme_dir)?;
        }

        // Watch site directory, ensuring it exists
        let site_dir = config.get_site_dir();
        fs::create_dir_all(&site_dir).unwrap();
        agent.watch(&site_dir)?;

        // Return file watcher
        Ok(Self { agent })
    }

    /// Returns whether the watcher is terminated.
    pub fn is_terminated(&self) -> bool {
        self.agent.is_terminated()
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Create identifier for the given path and sources.
///
/// This will also be hoisted into the file provider, which will make sure that
/// identifiers are platform independent by always ensuring forward slashes.
fn to_id(path: Arc<PathBuf>, sources: &[(PathBuf, String)]) -> Id {
    let option = sources.iter().find_map(|(prefix, context)| {
        if let Ok(suffix) = path.strip_prefix(prefix) {
            let location = suffix.to_str().unwrap_or("");
            Some(
                Id::builder()
                    .with_provider("file")
                    .with_context(context.replace('\\', "/"))
                    .with_location(location.replace('\\', "/"))
                    .build()
                    .expect("invariant"),
            )
        } else {
            None
        }
    });

    // Note that this cannot fail, since there must be a path in the source
    // mapping that matches the given path, at least the project root
    option.expect("invariant")
}
