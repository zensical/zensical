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

//! HTTP server.

use crossbeam::channel::{Receiver, TryRecvError};
use mio::net::{TcpListener, TcpStream};
use mio::{Interest, Token, Waker};
use slab::Slab;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::protocol::Role;
use tungstenite::{Error as WebSocketError, Message, WebSocket};

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
// Constants
// ----------------------------------------------------------------------------

/// Maximum number of active HTTP and WebSocket connections.
const MAX_CONNECTIONS: usize = 64;

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
    /// // Create server on an available local port
    /// let server = Server::new(Teapot, "127.0.0.1:0")?;
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

    /// Bound socket addresses are returned with assigned ephemeral ports.
    pub fn local_addrs(&self) -> Result<Vec<SocketAddr>> {
        self.acceptors
            .iter()
            .map(|listener| listener.local_addr().map_err(Into::into))
            .collect()
    }

    /// Polls the server for incoming events.
    ///
    /// The receiver is used to get notifications about file changes.
    #[allow(clippy::too_many_lines)]
    #[inline]
    pub fn poll(
        &mut self, receiver: Option<&Receiver<String>>,
    ) -> Result<bool> {
        if let Err(err) = self.events.poll(Some(Duration::from_secs(10)))
            && !matches!(
                err,
                Error::Io(ref io) if io.kind() == ErrorKind::Interrupted
            )
        {
            return Err(err);
        }

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
                            if self.connections.len() + self.clients.len()
                                >= MAX_CONNECTIONS
                            {
                                drop(socket);
                                continue;
                            }

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
                            // On Windows, occasionally, connection might have
                            // been closed or upgraded with pendng writes.
                            if let Some(conn) =
                                self.connections.get_mut(n - start)
                            {
                                if conn.is_writing() {
                                    interest |= Interest::WRITABLE;
                                }
                                self.events.reregister(
                                    conn.socket(),
                                    Token(n),
                                    interest,
                                )?;
                            }
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

        // WebSocket clients are not registered with the poller, so check
        // them after each poll to notice disconnected peers and process
        // control frames while the server is otherwise idle.
        self.clients.retain(|_, socket| websocket_is_alive(socket));

        // Keep on polling
        Ok(true)
    }

    // Return waker for waking server from poll loop
    pub fn waker(&self) -> Arc<Waker> {
        self.events.waker().clone()
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Checks a WebSocket peer and flushes any automatic control-frame response.
fn websocket_is_alive<T>(socket: &mut WebSocket<T>) -> bool
where
    T: Read + Write,
{
    let close = match socket.read() {
        Ok(Message::Close(_)) => true,
        Ok(_) => false,
        Err(WebSocketError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {
            return true;
        }
        Err(_) => return false,
    };

    match socket.flush() {
        Ok(()) => !close,
        Err(WebSocketError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {
            true
        }
        Err(_) => false,
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, ErrorKind, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
    use std::time::{Duration, Instant};
    use tungstenite::protocol::Role;
    use tungstenite::{Message, WebSocket};

    use crate::handler::Teapot;

    use super::connection::Connection;
    use super::{websocket_is_alive, Server, MAX_CONNECTIONS};

    struct WouldBlock;

    impl Read for WouldBlock {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::WouldBlock.into())
        }
    }

    impl Write for WouldBlock {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn websocket_pair() -> (TcpStream, WebSocket<mio::net::TcpStream>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        let socket = WebSocket::from_raw_socket(
            mio::net::TcpStream::from_std(stream),
            Role::Server,
            None,
        );

        (peer, socket)
    }

    fn wake_server<H>(server: &mut Server<H>) -> Duration
    where
        H: crate::handler::Handler,
    {
        let start = Instant::now();
        server.waker().wake().unwrap();
        assert!(server.poll(None).unwrap());
        start.elapsed()
    }

    fn wake_until_websocket_clients_removed<H>(server: &mut Server<H>)
    where
        H: crate::handler::Handler,
    {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !server.clients.is_empty() {
            let elapsed = wake_server(server);
            assert!(
                elapsed < Duration::from_secs(3),
                "server poll took {elapsed:?}"
            );
            assert!(
                Instant::now() < deadline,
                "websocket client was not removed before the deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn local_addrs_reports_assigned_port() {
        // An ephemeral port is requested on the loopback interface.
        let server = Server::new(Teapot, "127.0.0.1:0").unwrap();

        let addrs = server.local_addrs().unwrap();

        // The reported address must accept connections on its assigned port.
        assert_eq!(addrs.len(), 1);
        assert_ne!(addrs[0].port(), 0);
        let connection = TcpStream::connect(addrs[0]).unwrap();
        assert_eq!(connection.peer_addr().unwrap(), addrs[0]);
    }

    #[test]
    fn local_addrs_reports_all_listeners_in_bind_order() {
        // Each address to which localhost resolves is bound independently.
        let server = Server::new(Teapot, "localhost:0").unwrap();
        let mut expected = Vec::new();
        for addr in "localhost:0".to_socket_addrs().unwrap() {
            if !expected.contains(&addr.ip()) {
                expected.push(addr.ip());
            }
        }

        let addrs = server.local_addrs().unwrap();

        // All listeners must be reported in their resolved order.
        assert_eq!(
            addrs.iter().map(SocketAddr::ip).collect::<Vec<_>>(),
            expected
        );
        for addr in addrs {
            assert_ne!(addr.port(), 0);
            TcpStream::connect(addr).unwrap();
        }
    }

    #[test]
    fn drops_connections_above_the_active_connection_limit() {
        let mut server = Server::new(Teapot, "127.0.0.1:0").unwrap();
        let filler_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let filler_addr = filler_listener.local_addr().unwrap();
        let mut peers = Vec::with_capacity(MAX_CONNECTIONS);

        for _ in 0..MAX_CONNECTIONS {
            let peer = TcpStream::connect(filler_addr).unwrap();
            let (stream, _) = filler_listener.accept().unwrap();
            stream.set_nonblocking(true).unwrap();
            server
                .connections
                .insert(Connection::new(mio::net::TcpStream::from_std(stream)));
            peers.push(peer);
        }

        let mut excess =
            TcpStream::connect(server.local_addrs().unwrap()[0]).unwrap();
        excess.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let elapsed = wake_server(&mut server);
            assert!(elapsed < Duration::from_secs(3));

            let mut buffer = [0; 1];
            match excess.read(&mut buffer) {
                Ok(0) => break,
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => {
                    panic!("rejected connection failed unexpectedly: {err}")
                }
                Ok(_) => {
                    panic!("rejected connection unexpectedly received data")
                }
            }
        }

        assert_eq!(
            server.connections.len() + server.clients.len(),
            MAX_CONNECTIONS
        );
    }

    #[test]
    fn removes_websocket_clients_after_a_close_frame() {
        // A masked empty close frame from a WebSocket client.
        let frame = vec![0x88, 0x80, 1, 2, 3, 4];
        let mut socket =
            WebSocket::from_raw_socket(Cursor::new(frame), Role::Server, None);

        assert!(!websocket_is_alive(&mut socket));
    }

    #[test]
    fn removes_websocket_clients_after_eof() {
        let mut socket = WebSocket::from_raw_socket(
            Cursor::new(Vec::new()),
            Role::Server,
            None,
        );

        assert!(!websocket_is_alive(&mut socket));
    }

    #[test]
    fn retains_idle_nonblocking_websocket_clients() {
        let mut socket =
            WebSocket::from_raw_socket(WouldBlock, Role::Server, None);

        assert!(websocket_is_alive(&mut socket));
    }

    #[test]
    fn server_poll_keeps_open_idle_websocket_clients_without_blocking() {
        let mut server = Server::new(Teapot, "127.0.0.1:0").unwrap();
        let (_peer, socket) = websocket_pair();
        server.clients.insert(socket);

        let elapsed = wake_server(&mut server);

        assert!(elapsed < Duration::from_secs(3));
        assert_eq!(server.clients.len(), 1);
    }

    #[test]
    fn server_poll_removes_disconnected_websocket_clients_without_blocking() {
        let mut server = Server::new(Teapot, "127.0.0.1:0").unwrap();
        let (peer, socket) = websocket_pair();
        server.clients.insert(socket);
        drop(peer);

        wake_until_websocket_clients_removed(&mut server);
    }

    #[test]
    fn server_poll_removes_close_frame_clients_without_blocking() {
        let mut server = Server::new(Teapot, "127.0.0.1:0").unwrap();
        let (peer, socket) = websocket_pair();
        let mut peer = WebSocket::from_raw_socket(peer, Role::Client, None);
        peer.send(Message::Close(None)).unwrap();
        server.clients.insert(socket);

        wake_until_websocket_clients_removed(&mut server);
    }
}
