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

//! HTML-level excerpt transformations owned by the blog compatibility module.

use regex::Regex;
use std::sync::LazyLock;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Shifts and links rendered Markdown headings for a Material blog excerpt.
///
/// Material reparses each excerpt with a base heading level of two and anchor
/// links enabled. Reconstructing that small HTML-level difference lets the
/// native pipeline reuse the rendered post for every view appearance.
pub fn headings(input: &str, target: &str) -> (String, bool) {
    static HEADING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)<h([1-6])([^>]*)>(.*?)</h[1-6]>")
            .expect("static expression")
    });
    static ID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\bid=(?:\"([^\"]*)\"|'([^']*)'|([^\s>]+))"#)
            .expect("static expression")
    });
    static PERMALINK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?s)<a\s+[^>]*class=(?:\"[^\"]*\bheaderlink\b[^\"]*\"|'[^']*\bheaderlink\b[^']*')[^>]*>.*?</a>"#,
        )
        .expect("static expression")
    });

    let has_h1 = HEADING
        .captures_iter(input)
        .any(|captures| &captures[1] == "1" && ID.is_match(&captures[2]));
    let mut main_seen = !has_h1;
    let content = HEADING.replace_all(input, |captures: &regex::Captures<'_>| {
        let Some(id) = ID.captures(&captures[2]).and_then(|captures| {
            captures.get(1).or_else(|| captures.get(2)).or_else(|| captures.get(3))
        }) else {
            return captures[0].to_owned();
        };
        let level = captures[1]
            .parse::<u8>()
            .expect("heading expression captures a digit")
            .saturating_add(1)
            .min(6);
        let href = if main_seen {
            format!("{target}#{}", id.as_str())
        } else {
            main_seen = true;
            target.to_owned()
        };
        let title = PERMALINK.replace_all(&captures[3], "");
        format!(
            "<h{level}{}><a class=\"toclink\" href=\"{href}\">{title}</a></h{level}>",
            &captures[2]
        )
    });
    (content.into_owned(), has_h1)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::headings;

    #[test]
    fn shifts_and_links_headings_without_reparsing_markdown() {
        let input = concat!(
            r##"<h1 id="one">One<a class="headerlink" href="#one">¶</a></h1>"##,
            r##"<h2 id="two">Two<a class="headerlink" href="#two">¶</a></h2>"##,
            r##"<h6 id="six">Six<a class="headerlink" href="#six">¶</a></h6>"##,
        );
        assert_eq!(
            headings(input, "post/").0,
            concat!(
                r#"<h2 id="one"><a class="toclink" href="post/">One</a></h2>"#,
                r##"<h3 id="two"><a class="toclink" href="post/#two">Two</a></h3>"##,
                r##"<h6 id="six"><a class="toclink" href="post/#six">Six</a></h6>"##,
            )
        );
    }

    #[test]
    fn reserves_the_main_link_for_a_synthetic_heading() {
        let input =
            r##"<h2 id="two">Two<a class="headerlink" href="#two">¶</a></h2>"##;
        assert_eq!(
            headings(input, "post/").0,
            r#"<h3 id="two"><a class="toclink" href="post/#two">Two</a></h3>"#
        );
    }
}
