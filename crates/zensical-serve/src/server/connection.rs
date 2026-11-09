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

//! HTTP connection.

use mio::net::TcpStream;
use mio::Interest;
use std::io::{self, Cursor, ErrorKind, Read, Write};
use std::mem;
use std::time::Instant;
use tungstenite::protocol::WebSocketConfig;

use crate::handler::Handler;
use crate::http::request::Error;
use crate::http::response::ResponseExt;
use crate::http::{Header, Request, Response, Status};
use crate::server::Result;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Maximum size of a WebSocket frame or message received by the preview server.
const MAX_WEBSOCKET_SIZE: usize = 64 * 1024;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Connection action after handling an event
pub enum Signal {
    /// Continue with the specified interest.
    Interest(Interest),
    /// Continue without changing the current interest.
    Continue,
    /// Upgrade the connection.
    Upgrade(Upgrade),
    /// Connection was closed.
    Close,
}

/// Connection upgrade.
#[derive(Debug)]
pub enum Upgrade {
    /// Upgrade to WebSocket.
    WebSocket(WebSocketConfig),
}

// ----------------------------------------------------------------------------

/// Internal buffer state.
#[derive(Debug)]
enum Buffer {
    /// Currently reading data.
    Reading(Vec<u8>),
    /// Currently writing data, with optional upgrade or close-after-write.
    Writing(Cursor<Vec<u8>>, Option<Upgrade>, bool),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// HTTP connection.
#[derive(Debug)]
pub struct Connection {
    /// TCP socket.
    socket: TcpStream,
    /// Read/write buffer.
    buffer: Buffer,
    /// Last activity time.
    time: Instant,
    /// Start time of the current request.
    started: Instant,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Connection {
    /// Creates a connection.
    pub fn new(socket: TcpStream) -> Self {
        let now = Instant::now();
        Connection {
            socket,
            buffer: Buffer::Reading(Vec::new()),
            time: now,
            started: now,
        }
    }

    /// Consumes the connection and returns the underlying socket.
    pub fn into_socket(self) -> TcpStream {
        self.socket
    }

    /// Returns a mutable reference to the underlying socket.
    pub fn socket(&mut self) -> &mut TcpStream {
        &mut self.socket
    }

    /// Attempt to read data from the socket.
    #[allow(clippy::unnecessary_wraps)]
    pub fn read<H>(&mut self, handler: &H) -> Result<Signal>
    where
        H: Handler,
    {
        if let Buffer::Reading(buffer) = &mut self.buffer {
            self.time = Instant::now();
            // We try to read all remaining data - if the connection would
            // block, we return and wait for the next readable event
            let (res, upgrade, close) = {
                let mut temp = [0u8; 1024];
                match self.socket.read(&mut temp) {
                    Ok(0) => {
                        return Ok(Signal::Close);
                    }

                    // If we successfully read (some) bytes, try to parse and
                    // handle the request, or otherwise continue reading
                    Ok(bytes) => {
                        buffer.extend_from_slice(&temp[..bytes]);
                        match Request::from_bytes(buffer) {
                            // Request was parsed successfully, which means we
                            // process it, and switch to writing in order to
                            // return the response to the client. We also check
                            // if we need to switch protocols.
                            Ok(req) => {
                                let res = handler.handle(req);
                                let upgrade = (res.status
                                    == Status::SwitchingProtocols)
                                    .then_some(Upgrade::WebSocket(
                                        websocket_config(),
                                    ));
                                (res, upgrade, false)
                            }

                            // Request could not be parsed, as it is incomplete,
                            // so we keep reading
                            Err(Error::Incomplete) => {
                                return Ok(Signal::Interest(
                                    Interest::READABLE,
                                ));
                            }

                            // In case there was a validation error, return it
                            Err(Error::Validation(status)) => {
                                let res = Response::from_status(status)
                                    .header(Header::Connection, "close");
                                (res, None, true)
                            }

                            // If there was another parsing error, return 400
                            Err(_) => {
                                let res =
                                    Response::from_status(Status::BadRequest)
                                        .header(Header::Connection, "close");
                                (res, None, true)
                            }
                        }
                    }

                    // If the connection would block, return and wait for the
                    // next writable event to be available.
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        return Ok(Signal::Continue);
                    }

                    // In case of other errors, close the connection - for now
                    // well just print the error, and add proper handling later
                    Err(err) => {
                        return Ok(handle_socket_error(&err));
                    }
                }
            };

            // If we've processed all data, check if the request was an upgrade,
            // and if so, remember it to switch to the WebSocket protocol.
            let _ = mem::replace(
                &mut self.buffer,
                Buffer::Writing(Cursor::new(res.into_bytes()), upgrade, close),
            );
        }

        // Switch back to writing state
        Ok(Signal::Interest(Interest::WRITABLE))
    }

    /// Attempt to write data to the socket.
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::unnecessary_wraps)]
    pub fn write(&mut self) -> Result<Signal> {
        if let Buffer::Writing(cursor, _, _) = &mut self.buffer {
            self.time = Instant::now();
            // We try to write all remaining data - if the connection would
            // block, we return and wait for the next writable event
            loop {
                let pos = cursor.position() as usize;
                if pos >= cursor.get_ref().len() {
                    break;
                }

                // Attempt to write remaining bytes
                let buffer = cursor.get_ref();
                match self.socket.write(&buffer[pos..]) {
                    Ok(0) => {
                        return Ok(Signal::Close);
                    }

                    // If we successfully wrote some bytes, update the position
                    // and continue writing if there's more to send
                    Ok(bytes) => {
                        cursor.set_position((pos + bytes) as u64);
                    }

                    // If the connection would block, return and wait for the
                    // next writable event to be available.
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        return Ok(Signal::Continue);
                    }

                    // In case of other errors, close the connection - for now
                    // well just print the error, and add proper handling later
                    Err(err) => {
                        return Ok(handle_socket_error(&err));
                    }
                }
            }
        }

        // If we've written all data, check if the request was an upgrade, and
        // if so, return it to switch to the WebSocket protocol.
        let buffer =
            mem::replace(&mut self.buffer, Buffer::Reading(Vec::new()));
        match buffer {
            Buffer::Writing(_, Some(upgrade), _) => {
                return Ok(Signal::Upgrade(upgrade));
            }
            Buffer::Writing(_, None, true) => return Ok(Signal::Close),
            Buffer::Reading(_) | Buffer::Writing(_, None, false) => {}
        }

        // Switch back to reading state
        self.started = Instant::now();
        Ok(Signal::Interest(Interest::READABLE))
    }

    /// Returns whether the connection is currently writing data.
    pub fn is_writing(&self) -> bool {
        matches!(self.buffer, Buffer::Writing(_, _, _))
    }

    /// Check if connection has timed out
    pub fn is_timed_out(&self, now: Instant) -> bool {
        now.duration_since(self.time).as_secs() > 30
            || (matches!(&self.buffer, Buffer::Reading(_))
                && now.duration_since(self.started).as_secs() > 30)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn handle_socket_error(err: &io::Error) -> Signal {
    match err.kind() {
        ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof => {
            // Expected connection termination while reading or writing.
        }
        _ => {
            eprintln!("Error: {err}");
        }
    }

    Signal::Close
}

/// Creates a WebSocket configuration with bounded incoming frame and message
/// sizes.
fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_frame_size(Some(MAX_WEBSOCKET_SIZE))
        .max_message_size(Some(MAX_WEBSOCKET_SIZE))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        handle_socket_error, websocket_config, Connection, Signal,
        MAX_WEBSOCKET_SIZE,
    };
    use crate::handler::Teapot;
    use std::io::{Cursor, Error, ErrorKind, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};
    use tungstenite::protocol::Role;
    use tungstenite::{Error as WebSocketError, WebSocket};

    #[test]
    fn rejects_websocket_frame_exceeding_size_limit_from_header() {
        let payload_size = MAX_WEBSOCKET_SIZE + 1;
        let mut frame = vec![0x82, 0xff];
        frame.extend_from_slice(
            &u64::try_from(payload_size).unwrap().to_be_bytes(),
        );
        frame.extend_from_slice(&[0; 4]);
        let config = websocket_config();
        assert_eq!(config.max_frame_size, Some(MAX_WEBSOCKET_SIZE));
        assert_eq!(config.max_message_size, Some(MAX_WEBSOCKET_SIZE));
        let mut socket = WebSocket::from_raw_socket(
            Cursor::new(frame),
            Role::Server,
            Some(config),
        );

        assert!(matches!(socket.read(), Err(WebSocketError::Capacity(_))));
    }

    #[test]
    fn fatal_write_errors_close_connection() {
        for kind in [
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
            ErrorKind::UnexpectedEof,
        ] {
            assert!(matches!(
                handle_socket_error(&Error::from(kind)),
                Signal::Close
            ));
        }
    }

    #[test]
    fn malformed_request_gets_a_response_then_closes_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client =
            TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        let mut connection =
            Connection::new(mio::net::TcpStream::from_std(stream));

        client
            .write_all(b"POST / HTTP/1.1\r\nContent-Length: 1\r\n\r\nxy")
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match connection.read(&Teapot).unwrap() {
                Signal::Interest(interest) if interest.is_writable() => break,
                Signal::Interest(interest) if interest.is_readable() => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                Signal::Continue => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                _ => panic!("unexpected connection signal"),
            }
        }
        assert!(matches!(connection.write().unwrap(), Signal::Close));
        drop(connection);

        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert!(response.starts_with(b"HTTP/1.1 400 Bad Request\r\n"));
        assert!(response
            .windows(b"Connection: close".len())
            .any(|window| window == b"Connection: close"));
    }

    #[test]
    fn times_out_trickled_request_after_absolute_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client =
            TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        let mut connection =
            Connection::new(mio::net::TcpStream::from_std(stream));
        let now = Instant::now();
        connection.time = now;
        connection.started = now.checked_sub(Duration::from_secs(31)).unwrap();

        assert!(connection.is_timed_out(now));
        drop(client);
    }
}
