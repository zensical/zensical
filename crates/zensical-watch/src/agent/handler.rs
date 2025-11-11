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

//! File handler.

use crossbeam::channel::{after, never, select_biased, Receiver};
use notify::EventKind;
use std::mem;
use std::path::PathBuf;
use std::time::Duration;

use super::error::Result;
use super::event::Event;
use super::manager::Manager;
use super::monitor::Monitor;
use super::Action;

mod builder;

use builder::Builder;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// File handler.
pub struct Handler {
    /// Action receiver.
    receiver: Receiver<Action>, // replace with vector of handlers?
    /// Event handler.
    #[allow(clippy::struct_field_names)]
    handler: Box<dyn FnMut(Result<Event>) -> Result>,
    /// File monitor.
    monitor: Monitor,
    /// File manager.
    manager: Manager,
    /// Queue for paths from events.
    queue: Vec<PathBuf>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Handler {
    /// Creates a handler builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_watch::agent::Handler;
    ///
    /// // Create file handler builder
    /// let mut handler = Handler::builder();
    /// ```
    #[inline]
    #[must_use]
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Handles messages from the file agent and the file monitor.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip_all))]
    pub fn handle(&mut self, timeout: Duration) -> Result {
        // When receiving events from the file system, we debounce processing
        // by the given timeout, as we need to give the file system some time
        // to settle down. This ensures, that we can correctly handle renames,
        // as some file watcher backends send creation and removal events.
        let wait = (!self.queue.is_empty()).then_some(timeout);

        // Select over the receiver, which is the control channel for the file
        // agent, the monitor, the file watcher backend, and the timeout, which
        // is used to debounce events. Note that we use `select_biased` here to
        // prioritize ordering of processing.
        select_biased! {
            // Handle messages from the file agent, which are sent whenever the
            // owner instructs it to watch or unwatch a given path
            recv(self.receiver) -> message => {
                let res = match message? {
                    Action::Watch(path) => {
                        self.monitor.watch(&path).map(|_| {
                            self.queue.push(path);
                        })
                    },
                    Action::Unwatch(path) => {
                        self.monitor.unwatch(&path).map(|_| {
                            self.queue.push(path);
                        })
                    },
                };

                // Handle errors
                if let Err(err) = res {
                    (self.handler)(Err(err.into()))?;
                }
            }

            // Handle messages from the file monitor, which are sent whenever
            // a file system event is detected on a watched path
            recv(self.monitor.as_receiver()) -> message => {
                let res = message?.map(|event| {
                    self.queue.extend(filter(event.kind, event.paths));
                });
                if let Err(err) = res {
                    (self.handler)(Err(err.into()))?;
                }
            }

            // Handle timeouts, which are used to debounce events, and happen
            // when the queue isn't empty, and nothing happened for a time
            recv(wait.map_or_else(never, after)) -> _ => {
                let paths = mem::take(&mut self.queue);
                for res in self.manager.handle(paths) {
                    (self.handler)(res)?;
                }
            }
        }

        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Filters a file event.
///
/// This function normalizes the events emitted by [`notify`] to our [`Event`]
/// variant. Not all events are handled, as we only care about file and folder
/// creation, modification, and removal. Thus access events, and other events
/// are not emitted by the returned iterator.
#[inline]
fn filter<P>(kind: EventKind, paths: P) -> impl Iterator<Item = PathBuf>
where
    P: IntoIterator<Item = PathBuf>,
{
    paths.into_iter().filter_map(move |path| match kind {
        EventKind::Create(_) => Some(path),
        EventKind::Modify(_) => Some(path),
        EventKind::Remove(_) => Some(path),
        _ => None,
    })
}
