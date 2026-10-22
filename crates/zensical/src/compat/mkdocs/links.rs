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

//! Markdown link resolution over the shared HTML tokenizer.

use fluent_uri::component::Scheme;
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;

use super::html::{Editor, Visitor};
use super::url;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Relative Markdown URLs are resolved and rebased to the emitted page route.
pub struct Links<'a> {
    /// Whether Markdown pages are emitted as directories.
    directory_urls: bool,
    /// Whether a parent segment is required for the source page's route.
    prefix: bool,
    /// Original route against which source-relative URLs are resolved.
    from: &'a str,
    /// Emitted route from which relative URLs are generated.
    to: &'a str,
    /// Kind of URL attribute currently being read.
    attribute: Attribute,
}

enum Attribute {
    Other,
    Url,
    Srcset,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Links<'a> {
    /// A resolver is created for the source file and its emitted route.
    pub fn new(
        source: &str, directory_urls: bool, from: &'a str, to: &'a str,
    ) -> Self {
        Self {
            directory_urls,
            prefix: directory_urls
                && !matches!(name(source), "index.md" | "README.md"),
            from,
            to,
            attribute: Attribute::Other,
        }
    }

    fn rewrite_url(&self, value: &str) -> Option<String> {
        if !is_relative(value) {
            return None;
        }
        let mut suffix = value.find(['?', '#']).unwrap_or(value.len());
        // Parameters on the last path component are retained, matching
        // urllib.parse.urlparse in the former Python extension.
        let path = &value[..suffix];
        let component = path.rfind('/').map_or(0, |pos| pos + 1);
        if let Some(params) = path[component..].find(';') {
            suffix = component + params;
        }
        let (path, suffix) = value.split_at(suffix);
        let mut target = String::new();
        if self.prefix {
            target.push_str("../");
        }
        target.push_str(&markdown_path(path, self.directory_urls));
        target.push_str(suffix);

        if self.from != self.to {
            target = url::rebase(self.from, self.to, &target)?;
        }
        (target != value).then_some(target)
    }

    fn rewrite_srcset(&self, value: &str) -> Option<String> {
        let bytes = value.as_bytes();
        let mut output = String::new();
        let mut cursor = 0;
        let mut pos = 0;
        while pos < bytes.len() {
            // Commas within candidate URLs are retained, as in data URLs.
            while pos < bytes.len()
                && (whitespace(bytes[pos]) || bytes[pos] == b',')
            {
                pos += 1;
            }
            let start = pos;
            while pos < bytes.len() && !whitespace(bytes[pos]) {
                pos += 1;
            }
            let mut end = pos;
            while end > start && bytes[end - 1] == b',' {
                end -= 1;
            }
            if let Some(updated) = self.rewrite_url(&value[start..end]) {
                output.push_str(&value[cursor..start]);
                output.push_str(&updated);
                cursor = end;
            }

            // Descriptors are skipped, including commas within parentheses.
            if end == pos {
                let mut depth = 0_usize;
                while pos < bytes.len() {
                    match bytes[pos] {
                        b'(' => depth += 1,
                        b')' => depth = depth.saturating_sub(1),
                        b',' if depth == 0 => {
                            pos += 1;
                            break;
                        }
                        _ => {}
                    }
                    pos += 1;
                }
            }
        }
        if cursor == 0 {
            return None;
        }
        output.push_str(&value[cursor..]);
        Some(output)
    }
}

impl Visitor for Links<'_> {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { .. } => {
                self.attribute = Attribute::Other;
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute = match *name {
                    b"href" | b"src" => Attribute::Url,
                    b"srcset" => Attribute::Srcset,
                    _ => Attribute::Other,
                };
            }
            CallbackEvent::AttributeValue { value } => {
                // Original attribute bytes are retained so entities and quotes
                // are preserved. Encoded absolute URLs are excluded as well.
                let raw = editor.text(span.start..span.end);
                let updated = match self.attribute {
                    Attribute::Url
                        if is_relative(&String::from_utf8_lossy(value)) =>
                    {
                        self.rewrite_url(raw)
                    }
                    Attribute::Srcset => self.rewrite_srcset(raw),
                    _ => None,
                };
                if let Some(updated) = updated {
                    editor.replace(span.start..span.end, updated);
                }
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_relative(value: &str) -> bool {
    let value = value.trim_start_matches(|c: char| c <= ' ');
    if value.is_empty() || value.starts_with(['/', '?', '#']) {
        return false;
    }
    let prefix = value.split(['/', '?', '#']).next().unwrap_or(value);
    prefix
        .split_once(':')
        .is_none_or(|(scheme, _)| Scheme::new(scheme).is_none())
}

fn markdown_path(path: &str, directory_urls: bool) -> String {
    let trimmed = path.trim_end_matches('/');
    let Some(stem) = trimmed.strip_suffix(".md") else {
        return path.into();
    };
    let filename = name(trimmed);
    if directory_urls {
        if matches!(filename, "index.md" | "README.md") {
            trimmed[..trimmed.len() - filename.len()].into()
        } else {
            format!("{stem}/")
        }
    } else if filename == "README.md" {
        format!("{}index.html", &trimmed[..trimmed.len() - filename.len()])
    } else {
        format!("{stem}.html")
    }
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{markdown_path, Links};
    use crate::compat::mkdocs::html;

    #[test]
    fn markdown_paths_follow_mkdocs_routes() {
        for (path, directory, expected) in [
            ("index.md", true, ""),
            ("README.md", true, ""),
            ("guide/README.md", true, "guide/"),
            ("guide/page.md", true, "guide/page/"),
            ("myindex.md", true, "myindex/"),
            ("guide/README.md", false, "guide/index.html"),
            ("guide/page.md", false, "guide/page.html"),
            ("assets/app.js", true, "assets/app.js"),
            ("index.md/", true, ""),
            ("README.md/", true, ""),
            ("guide/index.md/", true, "guide/"),
            ("guide/README.md/", true, "guide/"),
            ("guide/page.md/", true, "guide/page/"),
            ("guide/index.md/", false, "guide/index.html"),
            ("guide/README.md/", false, "guide/index.html"),
            ("guide/page.md/", false, "guide/page.html"),
            ("assets/", true, "assets/"),
        ] {
            assert_eq!(markdown_path(path, directory), expected, "{path}");
        }
    }

    #[test]
    fn external_and_same_page_references_are_preserved() {
        let links =
            Links::new("guide/page.md", true, "guide/page/", "guide/page/");

        for value in [
            "https://example.com",
            "//example.com",
            "/root",
            "#section",
            "?diff1",
            "?diff1#section",
            "",
            "mailto:user@example.com",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
            " https://example.com",
            "\t/root",
        ] {
            assert_eq!(links.rewrite_url(value), None, "{value}");
        }
    }

    #[test]
    fn url_parameters_queries_and_fragments_are_preserved() {
        let links = Links::new("page.md", true, "page/", "page/");

        assert_eq!(
            links
                .rewrite_url("other.md;version=1?view=full#details")
                .as_deref(),
            Some("../other/;version=1?view=full#details"),
        );
        assert_eq!(
            links
                .rewrite_url("dir;v=1/other.md?view=full#details")
                .as_deref(),
            Some("../dir;v=1/other/?view=full#details"),
        );
    }

    #[test]
    fn attributes_are_rewritten_without_reserializing_html() {
        let source = concat!(
            r#"<div data-example='href="ignored.md"'>"#,
            r#"<a HREF = "target.md?view=1&amp;sort=2#part" title="unchanged">Target</a>"#,
            r#"<img SRC=assets/image.svg />"#,
            r#"<img src=é.png>"#,
            r#"<script src = 'app.js'>const s = 'src="ignored.js"';</script>"#,
            r#"<style>/* href="ignored.md" */</style></div>"#,
        );
        let mut links = Links::new("guide.md", true, "guide/", "guide/");

        let rendered = html::scan(source, &mut [&mut links]).unwrap();

        assert_eq!(
            rendered,
            source
                .replace("\"target.md?view=1", "\"../target/?view=1")
                .replace("SRC=assets/image.svg", "SRC=../assets/image.svg")
                .replace("src=é.png", "src=../é.png")
                .replace("src = 'app.js'", "src = '../app.js'")
        );
    }

    #[test]
    fn text_comments_and_raw_text_elements_are_preserved() {
        let mut links = Links::new("page.md", true, "page/", "page/");
        for source in [
            r#"<pre><code>src="$(pwd)" and href="target.md"</code></pre>"#,
            r#"<!-- <img src="image.png"> href="target.md" -->"#,
            r#"<script>const example = '<a href="target.md">';</script>"#,
            r#"<style>/* <img src="image.png"> */ a[href="target.md"] { color: red; }</style>"#,
            r#"<textarea><img src="image.png"> href="target.md"</textarea>"#,
            r#"<title>href="target.md"</title>"#,
            r#"<xmp><img src="image.png"></xmp>"#,
            r#"<iframe><a href="target.md">fallback</a></iframe>"#,
            r#"<div data-href="target.md" data-example='src="image.png"'>src="image.png"</div>"#,
            r#"<a href="&#47;root">Root</a><a href="&#35;local">Local</a>"#,
        ] {
            assert_eq!(html::scan(source, &mut [&mut links]), None, "{source}");
        }
    }

    #[test]
    fn srcset_candidates_keep_descriptors_separators_and_data_urls() {
        let mut links = Links::new("page.md", true, "page/", "page/");
        let source = concat!(
            r#"<source srcset="image-1.png 1x, image-2.png?raw=1&amp;next=2#image 2x, "#,
            r#"https://example.com/image.png 3x, data:image/png;base64,AAAA 4x">"#,
            r#"<img src='image.png' srcset='small.png, large.png 2x'>"#,
        );

        let rendered = html::scan(source, &mut [&mut links]).unwrap();

        assert_eq!(
            rendered,
            source
                .replace("image-1.png", "../image-1.png")
                .replace("image-2.png", "../image-2.png")
                .replace("src='image.png'", "src='../image.png'")
                .replace("small.png", "../small.png")
                .replace("large.png", "../large.png")
        );
    }

    #[test]
    fn relative_links_are_rebased_for_relocated_pages() {
        let mut links = Links::new(
            "blog/posts/hello.md",
            true,
            "blog/posts/hello/",
            "blog/2026/09/hello/",
        );
        let source = r##"<a href="../../notes.md?raw#part">Notes</a><img src="asset.png"><a href="#local">Local</a>"##;

        let rendered = html::scan(source, &mut [&mut links]).unwrap();

        assert_eq!(
            rendered,
            concat!(
                r#"<a href="../../../../notes/?raw#part">Notes</a>"#,
                r#"<img src="../../../posts/asset.png">"#,
                r##"<a href="#local">Local</a>"##,
            )
        );
    }
}
