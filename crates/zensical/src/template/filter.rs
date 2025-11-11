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

//! MiniJinja template filters.

use minijinja::{State, Value};
use std::fmt::Write;
use std::path::Path;
use zrx::path::PathExt;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// MiniJinja `url` filter.
///
/// This filter replicates the filter of the same name in MkDocs, resolving URLs
/// relative to the current page. If no page object is given, a static template
/// is rendered, which means that URLs must be resolved relative to base URL.
pub fn url_filter(state: &State, url: String) -> String {
    if url.starts_with('#') || url.starts_with('/') {
        return url;
    }

    // Leave absolute links unchanged
    if url.starts_with("http://") || url.starts_with("https://") {
        return url;
    }

    // Create target URL
    let target = Path::new(&url);

    // Render URLs in pages
    if let Some(source) = state
        .lookup("page")
        .and_then(|page| page.get_attr("url").ok())
        .filter(|value| !value.is_undefined())
        .map(|value| value.to_string())
    {
        // Make target URL relative to page
        target
            .relative_to(&source) // fmt
            .to_string_lossy()
            .replace('\\', "/")

    // Render URLs in static templates
    } else {
        let source = state.lookup("base_url").expect("invariant");
        Path::new(&source.to_string())
            .join(target.normalize())
            .to_string_lossy()
            .replace('\\', "/")
    }
}

/// MiniJinja `script_tag` filter.
///
/// This filter replicates the filter of the same name in MkDocs, generating a
/// script tag from a `extra_javascript` entry, which was introduced in MkDocs
/// 1.5.0 to allow the use of JavaScript files that are ESM modules. We always
/// convert to a structured format when during configuration parsing.
///
/// Note that MkDocs will set a missing path value to an empty string, which
/// is non-sensical, but we mirror behavior to stay compatible.
pub fn script_tag_filter(state: &State, value: Value) -> String {
    let path = value.get_attr("path").unwrap_or(Value::from(""));
    let mut html =
        format!("<script src=\"{}\"", url_filter(state, path.into()));

    // Set `type` attribute, if given
    if let Ok(kind) = value.get_attr("type") {
        if !kind.is_none() {
            write!(html, " type=\"{kind}\"").expect("invariant");
        }
    }

    // Set `async` attribute, if given
    if let Ok(flag) = value.get_attr("async") {
        if flag.is_true() {
            html.push_str(" async");
        }
    }

    // Set `defer` attribute, if given
    if let Ok(flag) = value.get_attr("defer") {
        if flag.is_true() {
            html.push_str(" defer");
        }
    }

    // Return script tag
    html.push_str("></script>");
    html
}
