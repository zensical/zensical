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

//! HTTP header.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use super::error::{Error, Result};

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl AsRef<str> for Header {
    /// Returns the string representation.
    #[inline]
    fn as_ref(&self) -> &str {
        self.name()
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Header {
    /// Formats the header for display.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.name())
    }
}

// ----------------------------------------------------------------------------
// Macros
// ----------------------------------------------------------------------------

/// Defines and implements HTTP headers.
macro_rules! define_and_impl_header {
    (
        $(
            // Header group
            $(#[$_:meta])*
            $group:ident:
            {
                $(
                    // Header definition
                    $(#[$comment:meta])*
                    $name:ident = $header:expr
                ),+
                $(,)?
            }
        )+
    ) => {
        /// HTTP header.
        ///
        /// This enum contains all common HTTP headers that can be used in a
        /// [`Request`][] or [`Response`][]. Be aware that it's an opinionated
        /// implementation, and should by no means be considered complete. It's
        /// solely intended for conveniently handling headers in middlewares.
        ///
        /// Also, consider the following headers:
        ///
        /// - [`Header::SetCookie`]
        /// - [`Header::ProxyAuthenticate`]
        /// - [`Header::WwwAuthenticate`]
        /// - [`Header::Trailer`]
        ///
        /// While the HTTP specification allows those specific headers to appear
        /// multiple times, our implementation only supports setting them once.
        ///
        /// [`Request`]: crate::connection::request::Request
        /// [`Response`]: crate::connection::response::Response
        #[allow(dead_code)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Header {
            $(
                $(
                    $(#[$comment])*
                    $name,
                )+
            )+
        }

        impl Header {
            /// Returns the header name.
            ///
            /// # Examples
            ///
            /// ```
            /// use zensical_serve::http::Header;
            ///
            /// // Create header
            /// let header = Header::ContentType;
            ///
            /// // Obtain header name
            /// assert_eq!(header.name(), "Content-Type");
            /// ```
            #[must_use]
            pub const fn name(&self) -> &'static str {
                match self {
                    $(
                        $(
                            Header::$name => $header,
                        )+
                    )+
                }
            }
        }

        /// Lookup table for HTTP headers (case-insensitive).
        static HEADER_LOOKUP_TABLE: LazyLock<HashMap<String, Header>> =
            LazyLock::new(|| {
                HashMap::from_iter([
                    $(
                        $(
                            ($header.to_lowercase(), Header::$name),
                        )+
                    )+
                ])
            });

        impl FromStr for Header {
            type Err = Error;

            /// Attempts to create a header from a string.
            ///
            /// # Errors
            ///
            /// This method returns [`Error::Header`], if the string does not
            /// match one of the known headers.
            ///
            /// # Examples
            ///
            /// ```
            /// # use std::error::Error;
            /// # fn main() -> Result<(), Box<dyn Error>> {
            /// use zensical_serve::http::Header;
            ///
            /// // Create header from string
            /// let header: Header = "Content-Type".parse()?;
            /// # Ok(())
            /// # }
            /// ```
            fn from_str(value: &str) -> Result<Self> {
                HEADER_LOOKUP_TABLE
                    .get(&value.to_lowercase())
                    .copied()
                    .ok_or_else(|| Error::Header(value.to_string()))
            }
        }
    }
}

// ----------------------------------------------------------------------------

define_and_impl_header! {

    /// General headers
    General: {
        /// Accept
        Accept = "Accept",
        /// Accept-Charset
        AcceptCharset = "Accept-Charset",
        /// Accept-Encoding
        AcceptEncoding = "Accept-Encoding",
        /// Accept-Language
        AcceptLanguage = "Accept-Language",
        /// Accept-Ranges
        AcceptRanges = "Accept-Ranges",
        /// Age
        Age = "Age",
        /// Allow
        Allow = "Allow",
        /// Alt-Svc
        AltSvc = "Alt-Svc",
        /// Authorization
        Authorization = "Authorization",
        /// Cache-Control
        CacheControl = "Cache-Control",
        /// Connection
        Connection = "Connection",
        /// Content-Disposition
        ContentDisposition = "Content-Disposition",
        /// Content-Encoding
        ContentEncoding = "Content-Encoding",
        /// Content-Language
        ContentLanguage = "Content-Language",
        /// Content-Length
        ContentLength = "Content-Length",
        /// Content-Location
        ContentLocation = "Content-Location",
        /// Content-Range
        ContentRange = "Content-Range",
        /// Content-Security-Policy
        ContentSecurityPolicy = "Content-Security-Policy",
        /// Content-Type
        ContentType = "Content-Type",
        /// Cookie
        Cookie = "Cookie",
        /// Date
        Date = "Date",
        /// ETag
        ETag = "ETag",
        /// Expect
        Expect = "Expect",
        /// Expires
        Expires = "Expires",
        /// Forwarded
        Forwarded = "Forwarded",
        /// From
        From = "From",
        /// Host
        Host = "Host",
        /// If-Match
        IfMatch = "If-Match",
        /// If-Modified-Since
        IfModifiedSince = "If-Modified-Since",
        /// If-None-Match
        IfNoneMatch = "If-None-Match",
        /// If-Range
        IfRange = "If-Range",
        /// If-Unmodified-Since
        IfUnmodifiedSince = "If-Unmodified-Since",
        /// Keep-Alive
        KeepAlive = "Keep-Alive",
        /// Last-Modified
        LastModified = "Last-Modified",
        /// Link
        Link = "Link",
        /// Location
        Location = "Location",
        /// Max-Forwards
        MaxForwards = "Max-Forwards",
        /// Origin
        Origin = "Origin",
        /// Pragma
        Pragma = "Pragma",
        /// Priority
        Priority = "Priority",
        /// Proxy-Authenticate
        ProxyAuthenticate = "Proxy-Authenticate",
        /// Proxy-Authorization
        ProxyAuthorization = "Proxy-Authorization",
        /// Range
        Range = "Range",
        /// Referer
        Referer = "Referer",
        /// Referrer-Policy
        ReferrerPolicy = "Referrer-Policy",
        /// Retry-After
        RetryAfter = "Retry-After",
        /// Server
        Server = "Server",
        /// Set-Cookie
        SetCookie = "Set-Cookie",
        /// Strict-Transport-Security
        StrictTransportSecurity = "Strict-Transport-Security",
        /// TE
        TE = "TE",
        /// Trailer
        Trailer = "Trailer",
        /// Transfer-Encoding
        TransferEncoding = "Transfer-Encoding",
        /// Upgrade
        Upgrade = "Upgrade",
        /// Upgrade-Insecure-Requests
        UpgradeInsecureRequests = "Upgrade-Insecure-Requests",
        /// User-Agent
        UserAgent = "User-Agent",
        /// Vary
        Vary = "Vary",
        /// Via
        Via = "Via",
        /// Warning
        Warning = "Warning",
        /// WWW-Authenticate
        WwwAuthenticate = "WWW-Authenticate",
    }

    /// CORS headers
    CrossOriginResourceSharing: {
        /// Access-Control-Allow-Credentials
        AccessControlAllowCredentials = "Access-Control-Allow-Credentials",
        /// Access-Control-Allow-Headers
        AccessControlAllowHeaders = "Access-Control-Allow-Headers",
        /// Access-Control-Allow-Methods
        AccessControlAllowMethods = "Access-Control-Allow-Methods",
        /// Access-Control-Allow-Origin
        AccessControlAllowOrigin = "Access-Control-Allow-Origin",
        /// Access-Control-Expose-Headers
        AccessControlExposeHeaders = "Access-Control-Expose-Headers",
        /// Access-Control-Max-Age
        AccessControlMaxAge = "Access-Control-Max-Age",
        /// Access-Control-Request-Headers
        AccessControlRequestHeaders = "Access-Control-Request-Headers",
        /// Access-Control-Request-Method
        AccessControlRequestMethod = "Access-Control-Request-Method",
    }

    /// Security headers
    Security: {
        /// X-Content-Type-Options
        XContentTypeOptions = "X-Content-Type-Options",
        /// X-DNS-Prefetch-Control
        XDnsPrefetchControl = "X-DNS-Prefetch-Control",
        /// X-Frame-Options
        XFrameOptions = "X-Frame-Options",
        /// X-XSS-Protection
        XXssProtection = "X-XSS-Protection",
    }

    /// Proxy headers
    Proxy: {
        /// X-Forwarded-For
        XForwardedFor = "X-Forwarded-For",
        /// X-Forwarded-Host
        XForwardedHost = "X-Forwarded-Host",
        /// X-Forwarded-Proto
        XForwardedProto = "X-Forwarded-Proto",
    }

    /// Fetch headers
    Fetch: {
        /// Sec-Fetch-Dest
        SecFetchDest = "Sec-Fetch-Dest",
        /// Sec-Fetch-Mode
        SecFetchMode = "Sec-Fetch-Mode",
        /// Sec-Fetch-Site
        SecFetchSite = "Sec-Fetch-Site",
        /// Sec-Fetch-User
        SecFetchUser = "Sec-Fetch-User",
        /// Sec-Purpose
        SecPurpose = "Sec-Purpose",
    }

    /// Client hint headers
    ClientHint: {
        /// Accept-CH
        AcceptClientHint = "Accept-CH",
        /// Sec-CH-UA
        SecClientHintUserAgent = "Sec-CH-UA",
        /// Sec-CH-UA-Mobile
        SecClientHintUserAgentMobile = "Sec-CH-UA-Mobile",
        /// Sec-CH-UA-Platform
        SecClientHintUserAgentPlatform = "Sec-CH-UA-Platform",
    }

    /// WebSocket headers
    WebSocket: {
        /// Sec-WebSocket-Accept
        SecWebSocketAccept = "Sec-WebSocket-Accept",
        /// Sec-WebSocket-Extensions
        SecWebSocketExtensions = "Sec-WebSocket-Extensions",
        /// Sec-WebSocket-Key
        SecWebSocketKey = "Sec-WebSocket-Key",
        /// Sec-WebSocket-Protocol
        SecWebSocketProtocol = "Sec-WebSocket-Protocol",
        /// Sec-WebSocket-Version
        SecWebSocketVersion = "Sec-WebSocket-Version",
    }

    /// Miscellaneous headers
    Miscellaneous: {
        /// X-Requested-With
        XRequestedWith = "X-Requested-With",
    }
}
