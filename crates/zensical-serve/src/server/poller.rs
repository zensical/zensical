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

//! Poller for I/O events.

use mio::event::{Event, Iter, Source};
use mio::{Events, Interest, Poll, Token, Waker};
use std::sync::Arc;
use std::time::Duration;

use super::error::Result;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Poller for I/O events.
pub struct Poller {
    /// Poll instance.
    poll: Poll,
    /// Event queue.
    events: Events,
    /// Waker.
    waker: Arc<Waker>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Poller {
    /// Creates a poller.
    pub fn new() -> Result<Self> {
        Self::with_capacity(1024)
    }

    /// Creates a poller with the given capacity.
    pub fn with_capacity(capacity: usize) -> Result<Self> {
        let res = Poll::new().and_then(|poll| {
            // The poller could be successfully created, so we can create the
            // waker by using the last token, which allows us to map connection
            // tokens to slab indices more easily, so we can efficiently manage
            // connections without further lookups
            let token = Token(usize::MAX);
            Waker::new(poll.registry(), token).map(|waker| Self {
                waker: Arc::new(waker),
                events: Events::with_capacity(capacity),
                poll,
            })
        });

        // Return poller or convert error
        res.map_err(Into::into)
    }

    /// Register a source for polling.
    #[inline]
    pub fn register<S>(
        &self, source: &mut S, token: Token, interest: Interest,
    ) -> Result
    where
        S: Source,
    {
        self.poll
            .registry()
            .register(source, token, interest)
            .map_err(Into::into)
    }

    /// Register a source for polling.
    #[inline]
    pub fn reregister<S>(
        &self, source: &mut S, token: Token, interest: Interest,
    ) -> Result
    where
        S: Source,
    {
        self.poll
            .registry()
            .reregister(source, token, interest)
            .map_err(Into::into)
    }

    /// Register a source for polling.
    #[inline]
    pub fn deregister<S>(&self, source: &mut S) -> Result
    where
        S: Source,
    {
        self.poll // fmt
            .registry()
            .deregister(source)
            .map_err(Into::into)
    }

    /// Waits for readiness events and returns the poller.
    #[inline]
    pub fn poll(&mut self, timeout: Option<Duration>) -> Result {
        self.poll
            .poll(&mut self.events, timeout)
            .map_err(Into::into)
    }

    /// Returns the waker.
    #[inline]
    #[must_use]
    pub fn waker(&self) -> Arc<Waker> {
        self.waker.clone()
    }

    /// Returns an iterator over the events.
    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        self.events.iter()
    }
}

#[allow(clippy::must_use_candidate)]
impl Poller {
    /// Returns the number of events.
    #[inline]
    pub fn len(&self) -> usize {
        self.events.iter().count()
    }

    /// Returns whether there are any events.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<'a> IntoIterator for &'a Poller {
    type Item = &'a Event;
    type IntoIter = Iter<'a>;

    /// Returns an iterator over the events.
    #[inline]
    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}
