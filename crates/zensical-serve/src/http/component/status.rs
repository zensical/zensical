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

//! HTTP status.

use std::fmt;

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl AsRef<str> for Status {
    /// Returns the string representation.
    #[inline]
    fn as_ref(&self) -> &str {
        self.name()
    }
}

// ----------------------------------------------------------------------------

impl fmt::Display for Status {
    /// Formats the status for display.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let code = *self as u16;
        f.write_str(code.to_string().as_str())?;
        f.write_str(" ")?;
        f.write_str(self.name())
    }
}

// ----------------------------------------------------------------------------
// Macros
// ----------------------------------------------------------------------------

/// Defines and implements HTTP status codes.
macro_rules! define_and_impl_status {
    (
        $(
            // Status group
            $(#[$_:meta])*
            $group:ident:
            {
                $(
                    // Status definition
                    $(#[$comment:meta])*
                    $name:ident = $code:expr, $reason:expr
                ),+
                $(,)?
            }
        )+
    ) => {
        /// HTTP status.
        #[allow(clippy::enum_variant_names)]
        #[allow(dead_code)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Status {
            $(
                $(
                    $(#[$comment])*
                    $name = $code,
                )+
            )+
        }

        impl Status {
            /// Returns the status name.
            ///
            /// # Examples
            ///
            /// ```
            /// use zensical_serve::http::Status;
            ///
            /// // Create status
            /// let status = Status::NotModified;
            ///
            /// // Obtain status name
            /// assert_eq!(status.name(), "Not Modified");
            /// ```
            #[must_use]
            pub const fn name(&self) -> &'static str {
                match self {
                    $(
                        $(
                            Status::$name => $reason,
                        )+
                    )+
                }
            }
        }
    };
}

// ----------------------------------------------------------------------------

define_and_impl_status! {

    /// 1xx Informational
    Informational: {
        /// 100 Continue
        Continue = 100, "Continue",
        /// 101 Switching Protocols
        SwitchingProtocols = 101, "Switching Protocols",
        /// 102 Processing
        Processing = 102, "Processing",
        /// 103 Early Hints
        EarlyHints = 103, "Early Hints",
    }

    /// 2xx Success
    Success: {
        /// 200 OK
        Ok = 200, "OK",
        /// 201 Created
        Created = 201, "Created",
        /// 202 Accepted
        Accepted = 202, "Accepted",
        /// 203 Non-Authoritative Information
        NonAuthoritativeInformation = 203, "Non-Authoritative Information",
        /// 204 No Content
        NoContent = 204, "No Content",
        /// 205 Reset Content
        ResetContent = 205, "Reset Content",
        /// 206 Partial Content
        PartialContent = 206, "Partial Content",
        /// 207 Multi-Status
        MultiStatus = 207, "Multi-Status",
        /// 208 Already Reported
        AlreadyReported = 208, "Already Reported",
        /// 226 IM Used
        ImUsed = 226, "IM Used",
    }

    /// 3xx Redirection
    Redirection: {
        /// 300 Multiple Choices
        MultipleChoices = 300, "Multiple Choices",
        /// 301 Moved Permanently
        MovedPermanently = 301, "Moved Permanently",
        /// 302 Found
        Found = 302, "Found",
        /// 303 See Other
        SeeOther = 303, "See Other",
        /// 304 Not Modified
        NotModified = 304, "Not Modified",
        /// 305 Use Proxy
        UseProxy = 305, "Use Proxy",
        /// 307 Temporary Redirect
        TemporaryRedirect = 307, "Temporary Redirect",
        /// 308 Permanent Redirect
        PermanentRedirect = 308, "Permanent Redirect",
    }

    /// 4xx Client Error
    ClientError: {
        /// 400 Bad Request
        BadRequest = 400, "Bad Request",
        /// 401 Unauthorized
        Unauthorized = 401, "Unauthorized",
        /// 402 Payment Required
        PaymentRequired = 402, "Payment Required",
        /// 403 Forbidden
        Forbidden = 403, "Forbidden",
        /// 404 Not Found
        NotFound = 404, "Not Found",
        /// 405 Method Not Allowed
        MethodNotAllowed = 405, "Method Not Allowed",
        /// 406 Not Acceptable
        NotAcceptable = 406, "Not Acceptable",
        /// 407 Proxy Authentication Required
        ProxyAuthenticationRequired = 407, "Proxy Authentication Required",
        /// 408 Request Timeout
        RequestTimeout = 408, "Request Timeout",
        /// 409 Conflict
        Conflict = 409, "Conflict",
        /// 410 Gone
        Gone = 410, "Gone",
        /// 411 Length Required
        LengthRequired = 411, "Length Required",
        /// 412 Precondition Failed
        PreconditionFailed = 412, "Precondition Failed",
        /// 413 Payload Too Large
        PayloadTooLarge = 413, "Payload Too Large",
        /// 414 URI Too Long
        UriTooLong = 414, "URI Too Long",
        /// 415 Unsupported Media Type
        UnsupportedMediaType = 415, "Unsupported Media Type",
        /// 416 Range Not Satisfiable
        RangeNotSatisfiable = 416, "Range Not Satisfiable",
        /// 417 Expectation Failed
        ExpectationFailed = 417, "Expectation Failed",
        /// 418 I'm a Teapot
        ImATeapot = 418, "I'm a Teapot",
        /// 421 Misdirected Request
        MisdirectedRequest = 421, "Misdirected Request",
        /// 422 Unprocessable Entity
        UnprocessableEntity = 422, "Unprocessable Entity",
        /// 423 Locked
        Locked = 423, "Locked",
        /// 424 Failed Dependency
        FailedDependency = 424, "Failed Dependency",
        /// 425 Too Early
        TooEarly = 425, "Too Early",
        /// 426 Upgrade Required
        UpgradeRequired = 426, "Upgrade Required",
        /// 428 Precondition Required
        PreconditionRequired = 428, "Precondition Required",
        /// 429 Too Many Requests
        TooManyRequests = 429, "Too Many Requests",
        /// 431 Request Header Fields Too Large
        RequestHeaderFieldsTooLarge = 431, "Request Header Fields Too Large",
        /// 451 Unavailable For Legal Reasons
        UnavailableForLegalReasons = 451, "Unavailable For Legal Reasons",
    }

    /// 5xx Server Error
    ServerError: {
        /// 500 Internal Server Error
        InternalServerError = 500, "Internal Server Error",
        /// 501 Not Implemented
        NotImplemented = 501, "Not Implemented",
        /// 502 Bad Gateway
        BadGateway = 502, "Bad Gateway",
        /// 503 Service Unavailable
        ServiceUnavailable = 503, "Service Unavailable",
        /// 504 Gateway Timeout
        GatewayTimeout = 504, "Gateway Timeout",
        /// 505 HTTP Version Not Supported
        HttpVersionNotSupported = 505, "HTTP Version Not Supported",
        /// 506 Variant Also Negotiates
        VariantAlsoNegotiates = 506, "Variant Also Negotiates",
        /// 507 Insufficient Storage
        InsufficientStorage = 507, "Insufficient Storage",
        /// 508 Loop Detected
        LoopDetected = 508, "Loop Detected",
        /// 510 Not Extended
        NotExtended = 510, "Not Extended",
        /// 511 Network Authentication Required
        NetworkAuthenticationRequired = 511, "Network Authentication Required",
    }
}
