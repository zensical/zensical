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

//! HTTP server.

use crossbeam::channel::{Receiver, TryRecvError};
use mio::net::{TcpListener, TcpStream};
use mio::{Interest, Token, Waker};
use slab::Slab;
use std::io::ErrorKind;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

use super::handler::{Handler, TryIntoHandler};
use super::server::connection::{Connection, Signal, Upgrade};

mod builder;
mod connection;
mod error;
mod poller;

pub use builder::Builder;
pub use error::{Error, Result};
use poller::Poller;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP server.
///
/// This implementation is still experimental and subject to change – it's not
/// finished yet, but should already work reliably for previews and reloads. We
/// plan to rework connection handling in the future once we start working on
/// additional server features.
pub struct Server<H>
where
    H: Handler,
{
    /// Handler for incoming requests.
    handler: H,
    /// Poller for I/O events.
    events: Poller,
    /// Acceptors for incoming connections.
    acceptors: Vec<TcpListener>,
    /// HTTP connections.
    connections: Slab<Connection>,
    /// WebSocket clients.
    clients: Slab<WebSocket<TcpStream>>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<H> Server<H>
where
    H: Handler,
{
    /// Creates a server.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::Teapot;
    /// use zensical_serve::server::Server;
    ///
    /// // Create server
    /// let server = Server::new(Teapot, "127.0.0.1:8080")?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn new<T, A>(handler: T, addr: A) -> Result<Self>
    where
        T: TryIntoHandler<Output = H>,
        A: ToSocketAddrs,
    {
        Self::builder(handler)?.bind(addr)?.listen()
    }

    /// Creates a server builder.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::Teapot;
    /// use zensical_serve::server::Server;
    ///
    /// // Create server builder
    /// let mut builder = Server::builder(Teapot)?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn builder<T>(handler: T) -> Result<Builder<H>>
    where
        T: TryIntoHandler<Output = H>,
    {
        Builder::new(handler)
    }

    /// Polls the server for incoming events.
    ///
    /// The receiver is used to get notifications about file changes.
    #[allow(clippy::too_many_lines)]
    #[inline]
    pub fn poll(
        &mut self, receiver: Option<&Receiver<String>>,
    ) -> Result<bool> {
        self.events.poll(Some(Duration::from_secs(10)))?;

        // Check if we need to clean up timed out connections
        let now = Instant::now();
        let mut timed_out = Vec::new();

        // Collect timed out connections
        for (n, conn) in &self.connections {
            if conn.is_timed_out(now) {
                timed_out.push(n);
            }
        }

        // Clean up timed out connections
        for n in timed_out {
            if let Some(conn) = self.connections.try_remove(n) {
                let mut socket = conn.into_socket();
                self.events.deregister(&mut socket)?;
            }
        }

        // Handle events
        let start = self.acceptors.len();
        for event in &self.events {
            let token = event.token();
            let n: usize = token.into();

            // Received a waker event
            if n == usize::MAX {
                if let Some(receiver) = receiver {
                    loop {
                        match receiver.try_recv() {
                            Ok(path) => {
                                self.clients.retain(|_, socket| {
                                    socket
                                        .send(Message::Text(
                                            path.clone().into(),
                                        ))
                                        .is_ok()
                                });
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                return Err(Error::Disconnected);
                            }
                        }
                    }
                }
                continue;
            }

            // Check if the event is for an acceptor or a connection
            if let Some(acceptor) = self.acceptors.get(n) {
                // Accept new connections - note that we need to run this in a
                // loop, as browsers might open several new connections at once
                loop {
                    match acceptor.accept() {
                        Ok((socket, _addr)) => {
                            let n = self
                                .connections
                                .insert(Connection::new(socket));
                            self.events.register(
                                self.connections[n].socket(),
                                Token(start + n),
                                Interest::READABLE,
                            )?;
                        }

                        // Everything else except would block is an error
                        Err(err) => {
                            if err.kind() != ErrorKind::WouldBlock {
                                eprintln!("Accept error: {err}");
                            }
                            break;
                        }
                    }
                }
            } else if let Some(conn) = self.connections.get_mut(n - start) {
                // Collect signals to process, which we do after processing all
                // events in order make the borrow checker happy
                let mut signals = Vec::new();
                if event.is_readable() {
                    signals.push((conn.read(&self.handler)?, n));
                }
                if event.is_writable() {
                    signals.push((conn.write()?, n));
                }

                // Handle signals after reading or writing on the socket - this
                // tells us what to do next with the connection
                for (signal, n) in signals {
                    match signal {
                        // Change of interest - reregister with poller
                        Signal::Interest(mut interest) => {
                            let conn = &mut self.connections[n - start];
                            if conn.is_writing() {
                                interest |= Interest::WRITABLE;
                            }
                            self.events.reregister(
                                conn.socket(),
                                Token(n),
                                interest,
                            )?;
                        }

                        // Close connection and deregister from poller
                        Signal::Close => {
                            let conn = self.connections.remove(n - start);
                            let mut socket = conn.into_socket();
                            self.events.deregister(&mut socket)?;
                        }

                        // Upgrade connection
                        Signal::Upgrade(upgrade) => {
                            let Upgrade::WebSocket(config) = upgrade;

                            // Remove connection from HTTP pool and handle as
                            // a WebSocket from now on. We currently don't
                            // support listening on WebSockets, but we'll add
                            // that later once we work on browser communication.
                            let conn = self.connections.remove(n - start);
                            let mut socket = conn.into_socket();
                            self.events.deregister(&mut socket)?;
                            self.clients.insert(WebSocket::from_raw_socket(
                                socket,
                                Role::Server,
                                Some(config),
                            ));
                        }

                        // Continue without changes
                        Signal::Continue => {}
                    }
                }
            }
        }

        // Keep on polling
        Ok(true)
    }

    // Return waker for waking server from poll loop
    pub fn waker(&self) -> Arc<Waker> {
        self.events.waker().clone()
    }
}
