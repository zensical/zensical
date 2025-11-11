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

//! HTTP connection.

use mio::net::TcpStream;
use mio::Interest;
use std::io::{Cursor, ErrorKind, Read, Write};
use std::mem;
use std::time::Instant;
use tungstenite::protocol::WebSocketConfig;

use crate::handler::Handler;
use crate::http::request::Error;
use crate::http::response::ResponseExt;
use crate::http::{Request, Response, Status};
use crate::server::Result;

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
    /// Currently writing data, with optional upgrade.
    Writing(Cursor<Vec<u8>>, Option<Upgrade>),
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
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Connection {
    /// Creates a connection.
    pub fn new(socket: TcpStream) -> Self {
        Connection {
            socket,
            buffer: Buffer::Reading(Vec::new()),
            time: Instant::now(),
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
            let (res, upgrade) = {
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
                                        WebSocketConfig::default(),
                                    ));
                                (res, upgrade)
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
                                let res = Response::from_status(status);
                                (res, None)
                            }

                            // If there was another parsing error, return 400
                            Err(_) => {
                                let res =
                                    Response::from_status(Status::BadRequest);
                                (res, None)
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
                        match err.kind() {
                            ErrorKind::ConnectionReset
                            | ErrorKind::ConnectionAborted
                            | ErrorKind::BrokenPipe
                            | ErrorKind::UnexpectedEof => {
                                // All of those are expected errors, so we just
                                // fall through here without printing anything
                            }
                            _ => {
                                eprintln!("Error: {err}");
                            }
                        }
                        return Ok(Signal::Close);
                    }
                }
            };

            // If we've processed all data, check if the request was an upgrade,
            // and if so, remember it to switch to the WebSocket protocol.
            let _ = mem::replace(
                &mut self.buffer,
                Buffer::Writing(Cursor::new(res.into_bytes()), upgrade),
            );
        }

        // Switch back to writing state
        Ok(Signal::Interest(Interest::WRITABLE))
    }

    /// Attempt to write data to the socket.
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::unnecessary_wraps)]
    pub fn write(&mut self) -> Result<Signal> {
        if let Buffer::Writing(cursor, _) = &mut self.buffer {
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
                        match err.kind() {
                            ErrorKind::ConnectionReset
                            | ErrorKind::ConnectionAborted
                            | ErrorKind::BrokenPipe
                            | ErrorKind::UnexpectedEof => {
                                // All of those are expected errors, so we just
                                // fall through here without printing anything
                            }
                            _ => {
                                eprintln!("Error: {err}");
                            }
                        }
                    }
                }
            }
        }

        // If we've written all data, check if the request was an upgrade, and
        // if so, return it to switch to the WebSocket protocol.
        let buffer =
            mem::replace(&mut self.buffer, Buffer::Reading(Vec::new()));
        if let Buffer::Writing(_, Some(upgrade)) = buffer {
            return Ok(Signal::Upgrade(upgrade));
        }

        // Switch back to reading state
        Ok(Signal::Interest(Interest::READABLE))
    }

    /// Returns whether the connection is currently writing data.
    pub fn is_writing(&self) -> bool {
        matches!(self.buffer, Buffer::Writing(_, _))
    }

    /// Check if connection has timed out
    pub fn is_timed_out(&self, now: Instant) -> bool {
        now.duration_since(self.time).as_secs() > 30
    }
}
