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

//! Middleware for livereload client.

use zensical_serve::handler::Handler;
use zensical_serve::http::{Header, Request, Response};
use zensical_serve::middleware::Middleware;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Livereload client script.
///
/// This script connects to the WebSocket server and listens for messages. When
/// a message is received, it will either reload the page or update a CSS file
/// dynamically to reflect changes without a full page reload, allowing for
/// very fast feedback loops when editing CSS files.
static CLIENT: &str = concat!(
    "(() => {\n",
    "  const title = document.title;\n",
    "  let closed = false;\n",
    "  function pending(state) {\n",
    "    document.title = state ? \"Waiting for connection\" : title;\n",
    "  }\n",
    "  function connect() {\n",
    "    const socket = new WebSocket(`ws://${window.location.host}`);\n",
    "    pending(true);\n",
    "    socket.addEventListener(\"message\", ev => {\n",
    "      if (ev.data.endsWith(\".css\")) {\n",
    "        const file = ev.data.split(\"/\").pop();\n",
    "        document.querySelectorAll(`link[rel=\"stylesheet\"]`)",
    "          .forEach(link => {\n",
    "            if (link.href.includes(file)) {\n",
    "              const reload = link.cloneNode(true);\n",
    "              reload.addEventListener(\"load\", () => {\n",
    "                link.parentNode.removeChild(link)\n",
    "              })\n",
    "            }\n",
    "          });\n",
    "        return\n",
    "      }\n",
    "      if (ev.data.endsWith(\".js\")) {\n",
    "        window.location.reload()\n",
    "      }\n",
    "      if (ev.data == window.location.pathname) {\n",
    "        window.location.reload()\n",
    "      }\n",
    "    });\n",
    "    socket.addEventListener(\"open\", () => {\n",
    "      setTimeout(() => pending(false), 100);\n",
    "      console.info(`Connected to ${socket.url}`)\n",
    "      if (closed) {\n",
    "        window.location.reload()\n",
    "      }\n",
    "    });\n",
    "    socket.addEventListener(\"close\", () => {\n",
    "      closed = true\n",
    "      setTimeout(() => connect(), 1000)\n",
    "    })\n",
    "  }\n",
    "  connect()\n",
    "})()\n"
);

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Middleware for livereload client.
#[derive(Default)]
pub struct Client;

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for Client {
    /// Processes the given request.
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        let uri = req.uri.path.clone();
        let mut res = next.handle(req);

        // In case an HTML file is served, inject the client script
        if let Some(value) = res.headers.get(Header::ContentType) {
            if value.contains("text/html") {
                res.body.extend(b"<script type=\"module\">");
                res.body.extend(CLIENT.as_bytes());
                res.body.extend(b"</script>");

                // Update content length
                res.headers.insert(Header::ContentLength, res.body.len());
            }
        }

        // Never cache JavaScript or CSS files, so reloading works smoothly
        if uri.ends_with(".js") || uri.ends_with(".css") {
            res.headers.insert(Header::CacheControl, "no-cache");
        }

        // Return response
        res
    }
}
