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

//! File agent.

use crossbeam::channel::{unbounded, Sender};
use std::path::{Path, PathBuf};
use std::thread::{Builder, JoinHandle};
use std::time::Duration;
use std::{fmt, fs};

mod error;
pub mod event;
mod handler;
mod manager;
mod monitor;

pub use error::{Error, Result};
pub use event::Event;
pub use handler::Handler;
pub use manager::Manager;
pub use monitor::{Kind, Monitor};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// File agent action.
#[derive(Debug)]
pub enum Action {
    /// Watch path.
    Watch(PathBuf),
    /// Unwatch path.
    Unwatch(PathBuf),
    // /// Refresh path.
    // Refresh(PathBuf),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// File agent.
pub struct Agent {
    /// Debounce timeout.
    timeout: Duration,
    /// Action sender.
    sender: Sender<Action>,
    /// Join handle for the agent thread.
    thread: JoinHandle<Result>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Agent {
    /// Creates a file agent.
    ///
    /// # Panics
    ///
    /// Panics if thread creation fails.
    pub fn new<F>(timeout: Duration, f: F) -> Self
    where
        F: FnMut(Result<Event>) -> Result + Send + 'static,
    {
        let (sender, receiver) = unbounded();
        let h = move || -> Result<()> {
            let mut handler = Handler::builder()
                .receiver(receiver)
                .handler(f)
                .monitor(Monitor::default())
                .build()?;

            // Start event loop, which will automatically exit when the file
            // agent is dropped, since the sender disconnects the receiver
            loop {
                handler.handle(timeout)?;
            }
        };

        // We deliberately use unwrap here, as the capability to spawn threads
        // is a fundamental requirement of the file agent
        let thread = Builder::new()
            .name(String::from("zrx/monitor"))
            .spawn(h)
            .unwrap();

        // Return file agent
        Self { timeout, sender, thread }
    }

    /// Watches the given path.
    ///
    /// This method submits an [`Action`] to watch the given path, which is
    /// processed in the next iteration of the agent's event loop.
    ///
    /// # Errors
    ///
    /// If action submission fails, [`Error::Disconnected`] is returned. This
    /// can practically never happen, as the channel is dropped on shutdown.
    /// Other than that, the given path must exist and be accessible, as it is
    /// canonicalized before being processed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use std::time::Duration;
    /// use zensical_watch::Agent;
    ///
    /// // Create file agent and start watching
    /// let agent = Agent::new(Duration::from_millis(20), |event| {
    ///     println!("Event: {:?}", event);
    ///     Ok(())
    /// });
    /// agent.watch(".")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn watch<P>(&self, path: P) -> Result
    where
        P: AsRef<Path>,
    {
        self.sender
            .send(Action::Watch(fs::canonicalize(path)?))
            .map_err(Into::into)
    }

    /// Unwatches the given path.
    ///
    /// This method submits an [`Action`] to unwatch the given path, which is
    /// processed in the next iteration of the file agent's event loop.
    ///
    /// # Errors
    ///
    /// If action submission fails, [`Error::Disconnected`] is returned. This
    /// can practically never happen, as the channel is dropped on shutdown.
    /// Other than that, the given path must exist and be accessible, as it is
    /// canonicalized before being processed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use std::time::Duration;
    /// use zensical_watch::Agent;
    ///
    /// // Create file agent and start watching
    /// let agent = Agent::new(Duration::from_millis(20), |event| {
    ///     println!("Event: {:?}", event);
    ///     Ok(())
    /// });
    /// agent.watch(".")?;
    ///
    /// // Stop watching
    /// agent.unwatch(".")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn unwatch<P>(&self, path: P) -> Result
    where
        P: AsRef<Path>,
    {
        self.sender
            .send(Action::Unwatch(fs::canonicalize(path)?))
            .map_err(Into::into)
    }

    /// Checks whether the agent thread has terminated.
    #[must_use]
    pub fn is_terminated(&self) -> bool {
        self.thread.is_finished()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl fmt::Debug for Agent {
    /// Formats the file agent for debugging.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Agent")
            .field("timeout", &self.timeout)
            .field("pending", &self.sender.len())
            .finish_non_exhaustive()
    }
}
