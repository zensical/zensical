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
    /// Explicit browser origins for reverse proxies.
    origins: Vec<Origin>,
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
#[derive(Clone, Debug, PartialEq, Eq)]
struct Authority {
    /// Host component.
    host: Host,
    /// Explicit port, if present.
    port: Option<u16>,
}

/// Scheme of a browser origin.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Scheme {
    Http,
    Https,
}

/// Browser origin, including its scheme and authority.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Origin {
    scheme: Scheme,
    authority: Authority,
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
            origins: Vec::new(),
        }
    }

    /// Allows external browser origins through reverse proxies.
    ///
    /// TLS, if any, ends at the proxy; the preview server continues to speak
    /// HTTP. Allowed origins do not restrict client IP addresses or prove that
    /// a request came through the proxy. Non-loopback listeners must be kept
    /// private through network-level access controls.
    pub fn with_external_origins(
        address: &str, origins: &[String],
    ) -> Result<Self, &'static str> {
        let mut trusted = Self::new(address);
        for origin in origins {
            trusted.origins.push(parse_origin(origin).ok_or(
                "external origin must be an HTTP or HTTPS origin without a path",
            )?);
        }
        Ok(trusted)
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

        let local = self.trusts(&host.host);
        let external =
            self.origins.iter().any(|origin| origin.matches_host(&host));
        if !local && !external {
            return Response::from_status(Status::BadRequest);
        }

        let is_websocket = req
            .headers
            .get(Header::Upgrade)
            .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"));
        if is_websocket && let Some(origin) = req.headers.get(Header::Origin) {
            let allowed = if external {
                parse_origin(origin).is_some_and(|origin| {
                    self.origins.iter().any(|external| {
                        external.matches_host(&host)
                            && origin.matches_origin(external)
                    })
                })
            } else {
                same_origin(origin, &host)
            };
            if !allowed {
                return Response::from_status(Status::BadRequest);
            }
        }

        next.handle(req)
    }
}

impl Origin {
    /// Compares browser origins, treating explicit default ports as equal.
    fn matches_origin(&self, other: &Self) -> bool {
        self.scheme == other.scheme
            && self.authority.host == other.authority.host
            && self.authority.port.unwrap_or(self.scheme.default_port())
                == other.authority.port.unwrap_or(other.scheme.default_port())
    }

    /// Checks that the Host header names this external origin.
    fn matches_host(&self, host: &Authority) -> bool {
        self.authority.host == host.host
            && self.authority.port.unwrap_or(self.scheme.default_port())
                == host.port.unwrap_or(self.scheme.default_port())
    }
}

impl Scheme {
    /// Returns the default HTTP port for this scheme.
    fn default_port(&self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
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

/// Parses an HTTP or HTTPS browser origin without a path.
fn parse_origin(value: &str) -> Option<Origin> {
    let (scheme, authority) = value.split_once("://")?;
    let scheme = if scheme.eq_ignore_ascii_case("http") {
        Scheme::Http
    } else if scheme.eq_ignore_ascii_case("https") {
        Scheme::Https
    } else {
        return None;
    };
    if authority.contains(['/', '?', '#']) {
        return None;
    }
    Some(Origin {
        scheme,
        authority: parse_authority(authority)?,
    })
}

/// Checks whether a WebSocket Origin matches the HTTP Host authority.
fn same_origin(origin: &str, host: &Authority) -> bool {
    let Some(origin) = parse_origin(origin) else {
        return false;
    };
    origin.scheme == Scheme::Http
        && origin.authority.host == host.host
        && origin.authority.port.unwrap_or(80) == host.port.unwrap_or(80)
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

    fn with_origins(
        address: &str, origins: &[&str],
    ) -> Result<TrustedHosts, &'static str> {
        let origins =
            origins.iter().map(ToString::to_string).collect::<Vec<_>>();
        TrustedHosts::with_external_origins(address, &origins)
    }

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

    #[test]
    fn external_origin_allows_a_local_proxy_and_https_websocket() {
        let middleware =
            with_origins("127.0.0.1:8000", &["https://docs.example.test"])
                .unwrap();

        let page = Request::new().header(Header::Host, "docs.example.test");
        assert_eq!(
            middleware.process(page, &NotFound).status,
            Status::NotFound
        );

        let websocket = Request::new()
            .header(Header::Host, "docs.example.test:443")
            .header(Header::Upgrade, "websocket")
            .header(Header::Origin, "https://docs.example.test");
        assert_eq!(
            middleware.process(websocket, &NotFound).status,
            Status::NotFound
        );

        let local = Request::new()
            .header(Header::Host, "localhost:8000")
            .header(Header::Upgrade, "websocket")
            .header(Header::Origin, "http://localhost:8000");
        assert_eq!(
            middleware.process(local, &NotFound).status,
            Status::NotFound
        );
    }

    #[test]
    fn external_origin_rejects_other_hosts_ports_and_origins() {
        let middleware =
            with_origins("localhost:8000", &["https://docs.example.test"])
                .unwrap();

        for host in ["other.example.test", "docs.example.test:8443"] {
            let request = Request::new().header(Header::Host, host);
            assert_eq!(
                middleware.process(request, &NotFound).status,
                Status::BadRequest
            );
        }

        for origin in [
            "http://docs.example.test",
            "https://other.example.test",
            "https://docs.example.test:8443",
            "null",
        ] {
            let request = Request::new()
                .header(Header::Host, "docs.example.test")
                .header(Header::Upgrade, "websocket")
                .header(Header::Origin, origin);
            assert_eq!(
                middleware.process(request, &NotFound).status,
                Status::BadRequest,
                "origin {origin:?} should not be accepted"
            );
        }

        let default = TrustedHosts::new("localhost:8000");
        let request = Request::new().header(Header::Host, "docs.example.test");
        assert_eq!(
            default.process(request, &NotFound).status,
            Status::BadRequest
        );
    }

    #[test]
    fn external_origin_accepts_network_binds_and_requires_an_exact_origin() {
        for address in ["0.0.0.0:8000", "192.0.2.1:8000"] {
            assert!(
                with_origins(address, &["https://docs.example.test"]).is_ok()
            );
        }
        for origin in [
            "https://docs.example.test/path",
            "https://user@docs.example.test",
            "https://docs.example.test?query",
            "ftp://docs.example.test",
        ] {
            assert!(with_origins("127.0.0.1:8000", &[origin]).is_err());
        }
    }

    #[test]
    fn multiple_external_origins_keep_hosts_and_origins_paired() {
        let middleware = with_origins(
            "127.0.0.1:8000",
            &["https://first.example.test", "https://second.example.test"],
        )
        .unwrap();

        for host in ["first.example.test", "second.example.test"] {
            let request = Request::new().header(Header::Host, host);
            assert_eq!(
                middleware.process(request, &NotFound).status,
                Status::NotFound
            );
            let request = Request::new()
                .header(Header::Host, host)
                .header(Header::Upgrade, "websocket")
                .header(Header::Origin, format!("https://{host}"));
            assert_eq!(
                middleware.process(request, &NotFound).status,
                Status::NotFound
            );
        }

        let crossed = Request::new()
            .header(Header::Host, "first.example.test")
            .header(Header::Upgrade, "websocket")
            .header(Header::Origin, "https://second.example.test");
        assert_eq!(
            middleware.process(crossed, &NotFound).status,
            Status::BadRequest
        );
    }

    #[test]
    fn external_origin_takes_precedence_over_a_local_host_alias() {
        let middleware =
            with_origins("127.0.0.1:8000", &["https://localhost:8443"])
                .unwrap();

        let proxied = Request::new()
            .header(Header::Host, "localhost:8443")
            .header(Header::Upgrade, "websocket")
            .header(Header::Origin, "https://localhost:8443");
        assert_eq!(
            middleware.process(proxied, &NotFound).status,
            Status::NotFound
        );

        let wrong_scheme = Request::new()
            .header(Header::Host, "localhost:8443")
            .header(Header::Upgrade, "websocket")
            .header(Header::Origin, "http://localhost:8443");
        assert_eq!(
            middleware.process(wrong_scheme, &NotFound).status,
            Status::BadRequest
        );

        let local = Request::new()
            .header(Header::Host, "localhost:8000")
            .header(Header::Upgrade, "websocket")
            .header(Header::Origin, "http://localhost:8000");
        assert_eq!(
            middleware.process(local, &NotFound).status,
            Status::NotFound
        );
    }
}
