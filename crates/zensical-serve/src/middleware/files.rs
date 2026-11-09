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

//! Middleware for serving static files.

use httpdate::parse_http_date;
use std::fs;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::handler::Handler;
use crate::http::response::ResponseExt;
use crate::http::{Header, Method, Request, Response, Status};
use crate::middleware::Middleware;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Middleware for serving static files.
///
/// Since the static files middleware is fallible during construction, we might
/// consider implementing [`TryIntoMiddleware`] for it later on.
///
/// [`TryIntoMiddleware`]: crate::middleware::TryIntoMiddleware
pub struct StaticFiles {
    /// Base path.
    base: PathBuf,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl StaticFiles {
    /// Creates a middleware for serving static files.
    pub fn new<P>(path: P) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        path.canonicalize().map(|base| Self { base })
    }

    /// Handle fallback cases (file not found, wrong method, etc.)
    fn fallback(&self, req: Request, next: &dyn Handler) -> Response {
        let res = next.handle(req);

        // In case the path was not found, try to load `404.html`
        if res.status == Status::NotFound
            && let Some(full) = self.resolve(Path::new("404.html"))
            && let Ok(res) = Response::from_file(full)
        {
            return res.status(Status::NotFound);
        }

        // Otherwise, return original request
        res
    }

    /// Resolves a path only when it remains inside the canonical site root.
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        let path = self.base.join(path).canonicalize().ok()?;
        path.starts_with(&self.base).then_some(path)
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Middleware for StaticFiles {
    /// Processes the given request.
    fn process(&self, req: Request, next: &dyn Handler) -> Response {
        if !matches!(req.method, Method::Get | Method::Head) {
            return self.fallback(req, next);
        }

        // Remove leading slash from path. In case the path ends with a slash,
        // add "index.html", so we can correctly resolve the associated file
        let path = PathBuf::from(req.uri.path.trim_start_matches('/'));
        let mut path = path;
        if req.uri.path.ends_with('/') {
            path.push("index.html");
        }

        // Canonicalization follows symlinks, so a file or directory linked
        // outside the configured site root is treated as not found.
        let Some(full) = self.resolve(&path) else {
            return self.fallback(req, next);
        };

        // Attempt to load file, or delegate to fallback
        let Ok(mut res) = Response::from_file(&full) else {
            return self.fallback(req, next);
        };

        // Ensure a date is always set, as required by HTTP/1.1
        res.headers
            .insert(Header::Date, httpdate::fmt_http_date(SystemTime::now()));

        // In case we received a head request, remove body - we should rather
        // make this more granular by just checking for the file
        if req.method == Method::Head {
            return res.body([]);
        }

        // Try to obtain and parse header from request
        let option = req.headers.get(Header::IfModifiedSince);
        let Ok(header) = option.map(parse_http_date).transpose() else {
            return res;
        };

        // In case we can both extract the date from the header and the file
        // system lookup is successful, check if we can just return a 304
        if let (Some(date), Ok(meta)) = (header, fs::metadata(full))
            && let Ok(mut last) = meta.modified()
        {
            // Subtract one second to account for rounding issues
            last -= Duration::from_secs(1);
            if date >= last {
                return Response::new()
                    .status(Status::NotModified)
                    .header(Header::ContentLength, 0);
            }
        }

        // Otherwise just return response
        res
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::handler::NotFound;
    use crate::http::{Request, Status};
    use crate::middleware::Middleware;

    use super::StaticFiles;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "zensical-serve-static-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn serves_files_inside_the_site_root() {
        let site = TestDir::new();
        fs::write(site.0.join("index.html"), b"site content").unwrap();
        let middleware = StaticFiles::new(&site.0).unwrap();

        let response = middleware.process(Request::new().uri("/"), &NotFound);

        assert_eq!(response.status, Status::Ok);
        assert_eq!(response.body, b"site content");
    }

    #[cfg(unix)]
    #[test]
    fn does_not_serve_symlinks_that_escape_the_site_root() {
        use std::os::unix::fs::symlink;

        let site = TestDir::new();
        let outside = TestDir::new();
        fs::write(outside.0.join("secret.txt"), b"outside secret").unwrap();
        symlink(outside.0.join("secret.txt"), site.0.join("secret.txt"))
            .unwrap();
        let middleware = StaticFiles::new(&site.0).unwrap();

        let response =
            middleware.process(Request::new().uri("/secret.txt"), &NotFound);

        assert_eq!(response.status, Status::NotFound);
        assert!(!response
            .body
            .windows(b"outside secret".len())
            .any(|window| window == b"outside secret"));
    }

    #[cfg(unix)]
    #[test]
    fn does_not_serve_an_escaping_404_symlink() {
        use std::os::unix::fs::symlink;

        let site = TestDir::new();
        let outside = TestDir::new();
        fs::write(outside.0.join("404.html"), b"outside page").unwrap();
        symlink(outside.0.join("404.html"), site.0.join("404.html")).unwrap();
        let middleware = StaticFiles::new(&site.0).unwrap();

        let response =
            middleware.process(Request::new().uri("/missing"), &NotFound);

        assert_eq!(response.status, Status::NotFound);
        assert!(!response
            .body
            .windows(b"outside page".len())
            .any(|window| window == b"outside page"));
    }
}
