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

//! Middleware for WebSocket handshakes.

use base64::prelude::*;
use sha1_smol::Sha1;

use crate::handler::Handler;
use crate::http::response::ResponseExt;
use crate::http::{Header, Method, Request, Response, Status};

use super::Middleware;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Middleware for WebSocket handshakes.
///
/// This middleware handles the WebSocket handshake process, ensuring that
/// the request meets the necessary criteria for a successful upgrade to
/// WebSocket. It checks for the presence of required headers, validates
/// the method, and generates the appropriate response headers.
///
/// # Examples
///
/// ```
/// # use std::error::Error;
/// # fn main() -> Result<(), Box<dyn Error>> {
/// use zensical_serve::handler::{Handler, Stack, TryIntoHandler};
/// use zensical_serve::http::{Header, Method, Request, Status};
/// use zensical_serve::middleware::WebSocketHandshake;
///
/// // Create stack with middleware
/// let stack = Stack::new()
///     .with(WebSocketHandshake::default())
///     .try_into_handler()?;
///
/// // Create request
/// let req = Request::new()
///     .method(Method::Get)
///     .header(Header::Connection, "Upgrade")
///     .header(Header::Upgrade, "websocket")
///     .header(Header::SecWebSocketKey, "dGhlIHNhbXBsZSBub25jZQ==")
///     .header(Header::SecWebSocketVersion, "13");
///
/// // Handle request with stack
/// let res = stack.handle(req);
/// assert_eq!(res.status, Status::SwitchingProtocols);
/// assert_eq!(res.headers.get(Header::Connection), Some("Upgrade"));
/// assert_eq!(res.headers.get(Header::Upgrade), Some("websocket"));
/// # Ok(())
/// # }
/// ```
#[derive(Default)]
pub struct WebSocketHandshake;

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl WebSocketHandshake {
    /// Creates a middleware for WebSocket handshakes.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_serve::middleware::WebSocketHandshake;
    ///
    /// // Create middleware
    /// let middleware = WebSocketHandshake::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for WebSocketHandshake {
    /// Processes the given request.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_serve::handler::{NotFound, Stack};
    /// use zensical_serve::http::{Header, Method, Request, Status};
    /// use zensical_serve::middleware::{Middleware, WebSocketHandshake};
    ///
    /// // Create middleware
    /// let middleware = WebSocketHandshake::default();
    ///
    /// // Create request
    /// let req = Request::new()
    ///     .method(Method::Get)
    ///     .header(Header::Connection, "Upgrade")
    ///     .header(Header::Upgrade, "websocket")
    ///     .header(Header::SecWebSocketKey, "dGhlIHNhbXBsZSBub25jZQ==")
    ///     .header(Header::SecWebSocketVersion, "13");
    ///
    /// // Handle request with middleware
    /// let res = middleware.process(req, &NotFound);
    /// assert_eq!(res.status, Status::SwitchingProtocols);
    /// assert_eq!(res.headers.get(Header::Connection), Some("Upgrade"));
    /// assert_eq!(res.headers.get(Header::Upgrade), Some("websocket"));
    /// # Ok(())
    /// # }
    /// ```
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        // Since we want to quickly forward requests that are not upgrades to
        // the next handler, we first check for presence of the upgrade header
        let Some(upgrade) = req.headers.get(Header::Upgrade) else {
            return next.handle(req);
        };

        // We're only interested in WebSocket upgrades, so again, forward all
        // other upgrade requests to the next handler. If the request is indeed
        // a WebSocket upgrade, from here on, we check all preconditions, and
        // return errors as per RFC in case they are not met.
        if !upgrade.eq_ignore_ascii_case("websocket") {
            return next.handle(req);
        }

        // 1. Ensure method is GET
        if req.method != Method::Get {
            return Response::from_status(Status::MethodNotAllowed)
                .header(Header::Allow, "GET");
        }

        // 2.1 Ensure connection header is present
        let Some(connection) = req.headers.get(Header::Connection) else {
            return Response::from_status(Status::BadRequest);
        };

        // 2.2 Ensure connection header contains upgrade
        let mut iter = connection.split(',').map(str::trim);
        if !iter.any(|value| value.eq_ignore_ascii_case("upgrade")) {
            return Response::from_status(Status::BadRequest);
        }

        // 3. Ensure WebSocket version is 13
        if Some("13") != req.headers.get(Header::SecWebSocketVersion) {
            return Response::from_status(Status::UpgradeRequired)
                .header(Header::Upgrade, "websocket")
                .header(Header::SecWebSocketVersion, "13");
        }

        // 4. Ensure WebSocket key is present
        let Some(key) = req.headers.get(Header::SecWebSocketKey) else {
            return Response::from_status(Status::BadRequest);
        };

        // Return response for WebSocket handshake
        let accept = generate_accept_key(key);
        Response::new()
            .status(Status::SwitchingProtocols)
            .header(Header::Upgrade, "websocket")
            .header(Header::Connection, "Upgrade")
            .header(Header::SecWebSocketAccept, accept)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Generates the accept key for the WebSocket handshake.
///
/// This follows RFC 6455 Section 4.2.2, which requires:
///
/// 1. Concatenating the client key with the GUID
/// 2. Computing the SHA-1 hash of the result
/// 3. Base64 encoding the hash
fn generate_accept_key<K>(key: K) -> String
where
    K: AsRef<[u8]>,
{
    let mut hasher = Sha1::new();
    hasher.update(key.as_ref());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    BASE64_STANDARD.encode(hasher.digest().bytes())
}
