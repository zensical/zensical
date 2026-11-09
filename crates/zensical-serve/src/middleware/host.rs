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

//! Middleware for validating preview server Host and WebSocket Origin headers.

use std::net::IpAddr;

use crate::handler::Handler;
use crate::http::response::ResponseExt;
use crate::http::{Header, Request, Response, Status};
use crate::middleware::Middleware;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Middleware that restricts requests to the configured preview host.
pub struct TrustedHosts {
    /// Configured bind host, if it could be parsed.
    configured: Option<Host>,
    /// Whether the server is bound to a wildcard address.
    allow_any_ip: bool,
    /// Whether loopback aliases are accepted.
    allow_loopback: bool,
}

/// Parsed host name or IP address.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Host {
    /// IP address.
    Ip(IpAddr),
    /// DNS host name.
    Name(String),
}

/// Parsed HTTP authority.
#[derive(Debug)]
struct Authority {
    /// Host component.
    host: Host,
    /// Explicit port, if present.
    port: Option<u16>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl TrustedHosts {
    /// Creates middleware for the configured bind address.
    ///
    /// Loopback bind addresses also accept `localhost` and loopback IP
    /// aliases. Wildcard bind addresses accept IP literal Host values, but
    /// reject arbitrary DNS names that could be used for DNS rebinding.
    /// Browser WebSocket requests must include a matching HTTP Origin; an
    /// absent Origin remains valid for non-browser clients.
    #[must_use]
    pub fn new(address: &str) -> Self {
        let configured = bind_host(address).and_then(parse_host);
        let allow_any_ip = matches!(
            &configured,
            Some(Host::Ip(address)) if address.is_unspecified()
        );
        let allow_loopback = allow_any_ip
            || matches!(&configured, Some(Host::Name(name)) if name == "localhost")
            || matches!(
                &configured,
                Some(Host::Ip(address)) if address.is_loopback()
            );

        Self {
            configured,
            allow_any_ip,
            allow_loopback,
        }
    }

    /// Checks whether the request host is trusted for this bind address.
    fn trusts(&self, host: &Host) -> bool {
        if self.allow_any_ip {
            return matches!(host, Host::Ip(_))
                || matches!(host, Host::Name(name) if name == "localhost");
        }

        if self.allow_loopback
            && (matches!(host, Host::Name(name) if name == "localhost")
                || matches!(host, Host::Ip(address) if address.is_loopback()))
        {
            return true;
        }

        self.configured.as_ref() == Some(host)
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for TrustedHosts {
    /// Rejects untrusted Host values and cross-origin WebSocket upgrades.
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        let Some(host) =
            req.headers.get(Header::Host).and_then(parse_authority)
        else {
            return Response::from_status(Status::BadRequest);
        };

        if !self.trusts(&host.host) {
            return Response::from_status(Status::BadRequest);
        }

        let is_websocket = req
            .headers
            .get(Header::Upgrade)
            .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"));
        if is_websocket
            && let Some(origin) = req.headers.get(Header::Origin)
            && !same_origin(origin, &host)
        {
            return Response::from_status(Status::BadRequest);
        }

        next.handle(req)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Extracts the host component from a `host:port` bind address.
fn bind_host(address: &str) -> Option<&str> {
    if let Some(address) = address.strip_prefix('[') {
        let (host, suffix) = address.split_once(']')?;
        let port = suffix.strip_prefix(':')?;
        port.parse::<u16>().ok()?;
        return Some(host);
    }

    let (host, port) = address.rsplit_once(':')?;
    if host.contains(':') || host.is_empty() {
        return None;
    }
    port.parse::<u16>().ok()?;
    Some(host)
}

/// Parses a host name or IP address and normalizes it for comparison.
fn parse_host(host: &str) -> Option<Host> {
    if let Ok(address) = host.parse::<IpAddr>() {
        return Some(Host::Ip(address));
    }

    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty()
        || !host.is_ascii()
        || host.bytes().any(|byte| {
            byte.is_ascii_whitespace()
                || byte.is_ascii_control()
                || matches!(
                    byte,
                    b'/' | b'?'
                        | b'#'
                        | b'@'
                        | b'\\'
                        | b':'
                        | b'['
                        | b']'
                        | b','
                )
        })
    {
        return None;
    }

    Some(Host::Name(host.to_ascii_lowercase()))
}

/// Parses an HTTP authority, including an optional numeric port.
fn parse_authority(value: &str) -> Option<Authority> {
    let (host, port) = if let Some(value) = value.strip_prefix('[') {
        let (host, suffix) = value.split_once(']')?;
        let host = parse_host(host)?;
        if !matches!(&host, Host::Ip(IpAddr::V6(_))) {
            return None;
        }
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':')?.parse::<u16>().ok()?)
        };
        (host, port)
    } else if let Some((host, port)) = value.rsplit_once(':') {
        if host.contains(':') {
            return None;
        }
        (parse_host(host)?, Some(port.parse::<u16>().ok()?))
    } else {
        (parse_host(value)?, None)
    };

    Some(Authority { host, port })
}

/// Checks whether a WebSocket Origin matches the HTTP Host authority.
fn same_origin(origin: &str, host: &Authority) -> bool {
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") {
        return false;
    }
    let default_port = 80;

    if authority.contains(['/', '?', '#']) {
        return false;
    }

    let Some(origin) = parse_authority(authority) else {
        return false;
    };
    origin.host == host.host
        && origin.port.unwrap_or(default_port)
            == host.port.unwrap_or(default_port)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::handler::NotFound;
    use crate::http::{Header, Request, Status};
    use crate::middleware::Middleware;

    use super::TrustedHosts;

    #[test]
    fn accepts_the_configured_host_and_loopback_aliases() {
        let middleware = TrustedHosts::new("localhost:8000");

        for host in ["localhost:8000", "127.0.0.1:8000", "[::1]:8000"] {
            let response = middleware
                .process(Request::new().header(Header::Host, host), &NotFound);
            assert_eq!(response.status, Status::NotFound);
        }
    }

    #[test]
    fn rejects_untrusted_hosts_even_when_the_origin_matches_them() {
        let middleware = TrustedHosts::new("localhost:8000");
        let request = Request::new()
            .header(Header::Host, "rebind.example:8000")
            .header(Header::Upgrade, "websocket")
            .header(Header::Origin, "http://rebind.example:8000");

        assert_eq!(
            middleware.process(request, &NotFound).status,
            Status::BadRequest
        );
    }

    #[test]
    fn rejects_cross_origin_websocket_upgrades() {
        let middleware = TrustedHosts::new("localhost:8000");
        for origin in [
            "http://attacker.example:8000",
            "https://localhost:8000",
            "null",
        ] {
            let request = Request::new()
                .header(Header::Host, "localhost:8000")
                .header(Header::Upgrade, "websocket")
                .header(Header::Origin, origin);

            assert_eq!(
                middleware.process(request, &NotFound).status,
                Status::BadRequest,
                "origin {origin:?} should not be accepted"
            );
        }
    }

    #[test]
    fn accepts_same_origin_websocket_upgrades() {
        let middleware = TrustedHosts::new("localhost:8000");
        let request = Request::new()
            .header(Header::Host, "LOCALHOST:8000")
            .header(Header::Upgrade, "WebSocket")
            .header(Header::Origin, "http://localhost:8000");

        assert_eq!(
            middleware.process(request, &NotFound).status,
            Status::NotFound
        );
    }

    #[test]
    fn wildcard_bind_accepts_ip_literals_but_not_arbitrary_names() {
        let middleware = TrustedHosts::new("0.0.0.0:8000");
        let trusted = middleware.process(
            Request::new().header(Header::Host, "192.168.1.20:8000"),
            &NotFound,
        );
        let untrusted = middleware.process(
            Request::new().header(Header::Host, "attacker.example:8000"),
            &NotFound,
        );

        assert_eq!(trusted.status, Status::NotFound);
        assert_eq!(untrusted.status, Status::BadRequest);

        let configured_wildcard = middleware.process(
            Request::new().header(Header::Host, "0.0.0.0:8000"),
            &NotFound,
        );
        assert_eq!(configured_wildcard.status, Status::NotFound);
    }

    #[test]
    fn rejects_requests_without_a_host_header() {
        let middleware = TrustedHosts::new("localhost:8000");
        assert_eq!(
            middleware.process(Request::new(), &NotFound).status,
            Status::BadRequest
        );
    }
}
