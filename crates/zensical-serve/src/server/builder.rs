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

//! HTTP server builder.

use mio::net::TcpListener;
use mio::{Interest, Token};
use slab::Slab;
use std::net::{SocketAddr, ToSocketAddrs};

use crate::handler::{Handler, TryIntoHandler};

use super::poller::Poller;
use super::{Error, Result, Server};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP server builder.
pub struct Builder<H> {
    /// Handler for incoming requests.
    handler: H,
    /// Socket addresses to bind to.
    addrs: Vec<SocketAddr>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<H> Builder<H>
where
    H: Handler,
{
    /// Creates a server builder.
    ///
    /// Note that the canonical way to create a [`Server`] is to invoke the
    /// [`Server::builder`] method, which creates an instance of [`Builder`].
    /// However, if only a single address needs to be bound, it can be done
    /// directly using the [`Server::new`] method.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::Teapot;
    /// use zensical_serve::server::Builder;
    ///
    /// // Create server builder
    /// let mut builder = Builder::new(Teapot)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new<T>(handler: T) -> Result<Self>
    where
        T: TryIntoHandler<Output = H>,
    {
        handler
            .try_into_handler()
            .map_err(Into::into)
            .map(|handler| Self { handler, addrs: Vec::new() })
    }

    /// Adds a socket address to bind to.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::Teapot;
    /// use zensical_serve::server::Builder;
    ///
    /// // Create server builder and add address
    /// let mut builder = Builder::new(Teapot)?;
    /// builder.bind("127.0.0.1:8080")?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn bind<A>(mut self, addr: A) -> Result<Self>
    where
        A: ToSocketAddrs,
    {
        // The underlying system call might returned the same socket address
        // multiple times, which is why we need to deduplicate them
        let addrs = addr.to_socket_addrs()?;
        for addr in addrs {
            if !self.addrs.contains(&addr) {
                self.addrs.push(addr);
            }
        }
        Ok(self)
    }

    /// Creates the server and binds to the configured addresses.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::Teapot;
    /// use zensical_serve::server::Builder;
    ///
    /// // Create server builder and bind to address
    /// let mut builder = Builder::new(Teapot)?;
    /// let server = builder
    ///     .bind("127.0.0.1:8080")?
    ///     .listen()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn listen(self) -> Result<Server<H>> {
        if self.addrs.is_empty() {
            return Err(Error::NoAddress);
        }

        // Create a new poller, then bind listeners to all configured addresses,
        // register them for event notifications, and create and return server
        Poller::new().and_then(|poller| {
            let iter = self.addrs.into_iter().enumerate();
            let iter = iter.map(|(n, addr)| {
                let mut listener = TcpListener::bind(addr)?;
                poller
                    .register(&mut listener, Token(n), Interest::READABLE)
                    .map(|()| listener)
            });

            // Collect listeners from iterator and return server
            iter.collect::<Result<_>>().map(|acceptors: Vec<_>| Server {
                handler: self.handler,
                events: poller,
                acceptors,
                connections: Slab::new(),
                clients: Slab::new(),
            })
        })
    }
}
