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

//! Routes.

use std::collections::BTreeMap;

use crate::handler::matcher::{Match, Matcher};
use crate::handler::Handler;
use crate::http::{Method, Request, Response};
use crate::middleware::Middleware;

use super::action::Action;

mod builder;

pub use builder::Builder;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Routes.
///
/// Matchers are compiled from a set of routes, which are stored in a tree-like
/// structure, implemented as part of the [`matchit`] crate. Each set of routes
/// is scoped to a specific request method, which is used to determine what to
/// check for when a request is received.
#[derive(Debug)]
pub struct Routes {
    /// Map methods to matchers.
    matchers: BTreeMap<Method, Matcher<Box<dyn Action>>>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Routes {
    /// Creates a routes builder.
    #[must_use]
    pub fn builder() -> Builder {
        Builder::new()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for Routes {
    /// Processes the given request.
    ///
    /// This method matches a given request against all registered routes. If
    /// a match is found, the corresponding action is called. If not, it is
    /// forwarded to the next handler, which can be another middleware or the
    /// final handler in the processing chain.
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        if let Some(routes) = self.matchers.get(&req.method) {
            // If path is borrowed, which is the normal case for parsing, this
            // will only clone the reference, not the contents of the string
            let path = req.uri.path.clone();

            // Next, we canonicalize the path by removing the trailing slash if
            // it's not the root path, as the path might have been normalized.
            // This is because the matcher doesn't support optional trailing
            // slashes, so routes are never allowed to end with a slash.
            let path = if path == "/" {
                path.as_ref()
            } else {
                path.trim_end_matches('/')
            };

            // Finally, we resolve the path against the matcher, and invoke the
            // corresponding action if it matches a registered route
            if let Some(Match { data: action, params }) = routes.resolve(path) {
                return action.handle(req, params);
            }
        }

        // Forward to next handler
        next.handle(req)
    }
}
