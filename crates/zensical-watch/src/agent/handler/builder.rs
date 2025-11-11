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

//! File handler builder.

use crossbeam::channel::Receiver;

use super::{Action, Event, Handler, Manager, Monitor, Result};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// File handler builder.
pub struct Builder {
    /// Action receiver.
    receiver: Option<Receiver<Action>>,
    /// Event handler.
    handler: Option<Box<dyn FnMut(Result<Event>) -> Result>>,
    /// File monitor.
    monitor: Option<Monitor>,
    /// File manager.
    manager: Option<Manager>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Builder {
    /// Creates a file handler builder.
    ///
    /// Note that the canonical way to create a [`Handler`] is to invoke the
    /// [`Handler::builder`] method, which creates an instance of [`Builder`].
    /// This is also why we don't implement [`Default`] - the builder itself
    /// should be considered an implementation detail.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_watch::agent::Handler;
    ///
    /// // Create file handler builder
    /// let mut handler = Handler::builder();
    /// ```
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            receiver: None,
            handler: None,
            monitor: None,
            manager: None,
        }
    }

    /// Sets the receiver for actions.
    pub fn receiver(mut self, receiver: Receiver<Action>) -> Self {
        self.receiver = Some(receiver);
        self
    }

    /// Sets the sender for messages.
    pub fn handler<F>(mut self, handler: F) -> Self
    where
        F: 'static + Send,
        F: FnMut(Result<Event>) -> Result,
    {
        self.handler = Some(Box::new(handler));
        self
    }

    /// Sets the file monitor.
    pub fn monitor(mut self, monitor: Monitor) -> Self {
        self.monitor = Some(monitor);
        self
    }

    /// Sets the file manager.
    pub fn manager(mut self, manager: Manager) -> Self {
        self.manager = Some(manager);
        self
    }

    /// Builds the file handler.
    pub fn build(self) -> Result<Handler> {
        let receiver = self.receiver.ok_or("Receiver is required").unwrap();
        let handler = self.handler.ok_or("Handler is required").unwrap();
        Ok(Handler {
            receiver,
            handler,
            monitor: self.monitor.unwrap_or_default(),
            manager: self.manager.unwrap_or_default(),
            queue: Vec::new(),
        })
    }
}
