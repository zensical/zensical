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

//! Middleware for livereload client.

use zensical_serve::handler::Handler;
use zensical_serve::http::{Header, Request, Response, Status};
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
    "    const url = new URL(window.location.href);\n",
    "    url.protocol = url.protocol === \"https:\" ? \"wss:\" : \"ws:\";\n",
    "    url.hash = \"\";\n",
    "    const socket = new WebSocket(url.href);\n",
    "    pending(true);\n",
    "    socket.addEventListener(\"message\", ev => {\n",
    "      if (ev.data.endsWith(\".css\")) {\n",
    "        document.querySelectorAll(`link[rel=\"stylesheet\"]`)",
    "          .forEach(link => {\n",
    "            if (link.href.includes(ev.data)) {\n",
    "              const href = link.href.split(\"?\")[0]\n",
    "              link.href = href + \"?t=\" + new Date().getTime();\n",
    "            }\n",
    "          });\n",
    "        return\n",
    "      }\n",
    "      if (ev.data.endsWith(\".js\")) {\n",
    "        window.location.reload()\n",
    "      }\n",
    "      if (ev.data == path) {\n",
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

/// Appends the livereload client for the requested path.
fn append_client(body: &mut Vec<u8>, path: &str) {
    let path = serde_json::to_string(path)
        .expect("request path could not be serialized")
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");

    body.extend(b"<script type=\"module\">const path = ");
    body.extend(path.as_bytes());
    body.extend(b";\n");
    body.extend(CLIENT.as_bytes());
    body.extend(b"</script>");
}

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
                append_client(&mut res.body, &uri);

                // Update content length
                res.headers.insert(Header::ContentLength, res.body.len());
            }
        }

        // Never cache JavaScript or CSS files, so reloading works smoothly
        if uri.ends_with(".js") || uri.ends_with(".css") {
            res.headers.insert(Header::CacheControl, "no-cache");
        }

        // In case of a 404 on "/", we attach the WebSocket script, so it will
        // automatically reload once the build has finished. This is temporary,
        // since we're working on properly integrating all moving parts of
        // the system into a coherent flow.
        if res.status == Status::NotFound {
            res.body.clear();
            append_client(&mut res.body, &uri);

            // Update content length
            res.headers.insert(Header::ContentType, "text/html");
            res.headers.insert(Header::ContentLength, res.body.len());
        }

        // Return response
        res
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::str;

    use zensical_serve::http::Response;

    use super::*;

    #[test]
    fn client_uses_browser_scheme_host_and_path() {
        let req = Request::new().uri("/preview/guide/");
        let next = |_: Request| {
            Response::new()
                .header(Header::ContentType, "text/html")
                .body("content")
        };
        let res = Client.process(req, &next);
        let body = str::from_utf8(&res.body).unwrap();

        assert!(body.contains("const path = \"/preview/guide/\";"));
        assert!(body.contains("new URL(window.location.href)"));
        assert!(body.contains("? \"wss:\" : \"ws:\""));
        assert!(body.contains("new WebSocket(url.href)"));
        assert!(!body.contains("`ws://${window.location.host}`"));
        let length = res.body.len().to_string();
        assert_eq!(
            res.headers.get(Header::ContentLength),
            Some(length.as_str())
        );
    }

    #[test]
    fn client_safely_encodes_server_visible_path() {
        let req = Request::new().uri("/</script>?ignored=true");
        let res = Client.process(req, &|_: Request| {
            Response::new().status(Status::NotFound)
        });
        let body = str::from_utf8(&res.body).unwrap();

        assert!(body.contains("const path = \"/\\u003c/script\\u003e\";"));
        assert_eq!(body.matches("</script>").count(), 1);
    }
}
