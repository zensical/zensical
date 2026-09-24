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

//! Shared HTML processing for MkDocs-compatible plugins.

use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Span, Tokenizer};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::ops::Range;

use super::url;

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

/// Page-local observer participating in the shared HTML pass.
pub trait Visitor {
    /// Observes one tokenizer event and optionally records an output edit.
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    );
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Deferred edits to the HTML currently being scanned.
pub struct Editor<'a> {
    /// Original HTML input.
    input: &'a str,
    /// Edits recorded by visitors.
    edits: Vec<Edit>,
    /// Whether edits are ordered by their source ranges.
    sorted: bool,
}

/// Rewrites page-relative link and media targets between route bases.
struct RebaseUrls<'a> {
    /// Route against which current relative URLs are resolved.
    from: &'a str,
    /// Route from which rewritten relative URLs are emitted.
    to: &'a str,
    /// Optional target used for fragment-only links.
    fragment_base: Option<&'a str>,
    /// Whether the tokenizer is currently reading an attribute value.
    attribute: bool,
    /// Whether the current attribute is an `href` rather than a `src`.
    href: bool,
}

/// Rewrites resource source paths to their emitted public paths.
struct RewriteUrls<'a> {
    /// Route against which relative resource URLs are resolved.
    base: &'a str,
    /// Source-to-public resource path mapping.
    mappings: &'a HashMap<String, String>,
    /// Whether the tokenizer is currently reading a rewritable attribute.
    attribute: bool,
}

/// Collects local URLs addressed by rendered page content.
struct LocalTargets<'a> {
    /// Route against which relative URLs are resolved.
    base: &'a str,
    /// Whether the tokenizer is reading a local link or media target.
    attribute: bool,
    /// Resolved URLs without query parameters or fragments.
    targets: HashSet<String>,
}

/// One replacement in the original HTML input.
#[derive(Debug, PartialEq, Eq)]
struct Edit {
    /// Byte range replaced by this edit.
    range: Range<usize>,
    /// Replacement HTML.
    replacement: Box<str>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Editor<'a> {
    /// Scans HTML once and retains edits for completion after visitors finish.
    pub fn scan(input: &'a str, visitors: &mut [&mut dyn Visitor]) -> Self {
        let mut editor = Self {
            input,
            edits: Vec::new(),
            sorted: true,
        };
        {
            let mut emitter = CallbackEmitter::new(
                |event: CallbackEvent<'_>, span: Span<usize>| {
                    for visitor in &mut *visitors {
                        visitor.visit(&event, span, &mut editor);
                    }
                    None::<Infallible>
                },
            );
            emitter.naively_switch_states(true);

            Tokenizer::new_with_emitter(input, emitter)
                .finish()
                .expect("string input is infallible");
        }
        editor
    }

    /// Returns original HTML covered by a tokenizer span.
    pub fn text(&self, range: Range<usize>) -> &str {
        &self.input[range]
    }

    /// Returns whether the original HTML contains a prospective marker.
    pub fn contains(&self, value: &str) -> bool {
        self.input.contains(value)
    }

    /// Replaces a byte range after all visitors have observed the input.
    pub fn replace(
        &mut self, range: Range<usize>, replacement: impl Into<Box<str>>,
    ) {
        assert!(range.start <= range.end && range.end <= self.input.len());
        self.edits.push(Edit {
            range,
            replacement: replacement.into(),
        });
        self.sorted = false;
    }

    /// Removes the complete attribute whose name occupies `span`.
    pub fn remove_attribute(&mut self, name: &[u8], span: Span<usize>) {
        let bytes = self.input.as_bytes();
        assert!(span.start <= span.end && span.end <= bytes.len());

        // Attribute-name spans exclude the whitespace preceding the name.
        // Consume it so removing an attribute doesn't leave malformed or
        // needlessly expanded start tags behind.
        let mut start = span.start;
        while start > 0 && is_whitespace(bytes[start - 1]) {
            start -= 1;
        }

        // Attribute-value spans exclude whitespace, the equals sign, and
        // quotes. Recover that syntax directly from the original input so
        // boolean, quoted, and unquoted attributes share the same operation.
        // html5gum's attribute-name end can point at the byte that caused the
        // tokenizer to flush the name. The decoded name length gives us the
        // exact boundary for the ASCII compatibility attributes we remove.
        let mut end = span.start + name.len();
        let mut equals = end;
        skip_whitespace(bytes, &mut equals);
        if bytes.get(equals) == Some(&b'=') {
            end = equals + 1;
            skip_whitespace(bytes, &mut end);
            match bytes.get(end).copied() {
                Some(quote @ (b'\'' | b'"')) => {
                    end += 1;
                    while end < bytes.len() && bytes[end] != quote {
                        end += 1;
                    }
                    if end < bytes.len() {
                        end += 1;
                    }
                }
                Some(_) => {
                    while end < bytes.len()
                        && !is_whitespace(bytes[end])
                        && bytes[end] != b'>'
                    {
                        end += 1;
                    }
                }
                None => {}
            }
        }

        self.replace(start..end, Box::default());
    }

    /// Renders edits contained in a source range without consuming them.
    ///
    /// An enclosing replacement is excluded, allowing a visitor to retain
    /// edited inner content when it replaces the surrounding element.
    pub fn render_range(&mut self, range: Range<usize>) -> Option<String> {
        assert!(range.start <= range.end && range.end <= self.input.len());
        if !self.sorted {
            // Sort once, then use binary searches for individual reference
            // titles so pages with many references do not rescan every edit.
            self.edits.sort_by(|left, right| {
                left.range
                    .start
                    .cmp(&right.range.start)
                    .then_with(|| right.range.end.cmp(&left.range.end))
            });
            self.sorted = true;
        }

        // Outer edits sort before edits they contain. An outer replacement
        // owns its complete input span, while partially overlapping edits are
        // always a programming error between visitors.
        let start = self
            .edits
            .partition_point(|edit| edit.range.start < range.start);
        let end = self
            .edits
            .partition_point(|edit| edit.range.start <= range.end);
        let mut edits: Vec<&Edit> = Vec::new();
        for edit in &self.edits[start..end] {
            if edit.range.end > range.end {
                continue;
            }
            if let Some(previous) = edits.last()
                && edit.range.start < previous.range.end
            {
                assert!(
                    edit.range.end <= previous.range.end,
                    "HTML edits partially overlap"
                );
                if edit.range == previous.range {
                    assert_eq!(
                        edit.replacement, previous.replacement,
                        "HTML edits disagree on the same span"
                    );
                }
                continue;
            }
            edits.push(edit);
        }
        if edits.is_empty() {
            return None;
        }

        let removed = edits.iter().map(|edit| edit.range.len()).sum::<usize>();
        let inserted = edits
            .iter()
            .map(|edit| edit.replacement.len())
            .sum::<usize>();
        let mut output = String::with_capacity(
            range.len().saturating_sub(removed) + inserted,
        );
        let mut cursor = range.start;
        for edit in edits {
            assert!(self.input.is_char_boundary(edit.range.start));
            assert!(self.input.is_char_boundary(edit.range.end));
            output.push_str(&self.input[cursor..edit.range.start]);
            output.push_str(&edit.replacement);
            cursor = edit.range.end;
        }
        output.push_str(&self.input[cursor..range.end]);
        Some(output)
    }

    /// Applies all deferred edits in one linear output pass.
    pub fn finish(mut self) -> Option<String> {
        self.render_range(0..self.input.len())
    }
}

impl Visitor for RebaseUrls<'_> {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { .. } => {
                self.attribute = false;
                self.href = false;
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute = matches!(*name, b"href" | b"src");
                self.href = *name == b"href";
            }
            CallbackEvent::AttributeValue { value } if self.attribute => {
                let value = String::from_utf8_lossy(value);
                if self.href
                    && value.starts_with('#')
                    && let Some(base) = self.fragment_base
                {
                    editor.replace(
                        span.start..span.end,
                        format!("{base}{value}"),
                    );
                } else if let Some(value) =
                    url::rebase(self.from, self.to, &value)
                {
                    editor.replace(span.start..span.end, value);
                }
            }
            _ => {}
        }
    }
}

impl Visitor for RewriteUrls<'_> {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { .. } => self.attribute = false,
            CallbackEvent::AttributeName { name } => {
                self.attribute = matches!(*name, b"href" | b"src");
            }
            CallbackEvent::AttributeValue { value } if self.attribute => {
                let value = String::from_utf8_lossy(value);
                let Some(target) = url::resolve(self.base, &value) else {
                    return;
                };
                let suffix = target.find(['?', '#']).unwrap_or(target.len());
                let (path, suffix) = target.split_at(suffix);
                let Some(path) = self.mappings.get(path) else {
                    return;
                };
                editor.replace(
                    span.start..span.end,
                    url::relative(self.base, &format!("{path}{suffix}")),
                );
            }
            _ => {}
        }
    }
}

impl Visitor for LocalTargets<'_> {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, _span: Span<usize>,
        _editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { .. } => self.attribute = false,
            CallbackEvent::AttributeName { name } => {
                self.attribute = matches!(*name, b"href" | b"src");
            }
            CallbackEvent::AttributeValue { value } if self.attribute => {
                let value = String::from_utf8_lossy(value);
                if let Some(target) = url::resolve(self.base, &value) {
                    let end = target.find(['?', '#']).unwrap_or(target.len());
                    self.targets.insert(target[..end].to_owned());
                }
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Scans HTML once with all page-local visitors.
///
/// Returns modified HTML only when a visitor recorded an edit, allowing the
/// caller to retain the original allocation for observational passes.
pub fn scan(input: &str, visitors: &mut [&mut dyn Visitor]) -> Option<String> {
    Editor::scan(input, visitors).finish()
}

/// Rebases local `href` and `src` attributes between two page routes.
pub fn rebase_urls(input: &str, from: &str, to: &str) -> Option<String> {
    if from == to {
        return None;
    }
    let mut visitor = RebaseUrls {
        from,
        to,
        fragment_base: None,
        attribute: false,
        href: false,
    };
    scan(input, &mut [&mut visitor])
}

/// Rebases URLs and prefixes fragment-only links with a separate base.
pub fn rebase_urls_with_fragment_base(
    input: &str, from: &str, to: &str, fragment_base: &str,
) -> Option<String> {
    let mut visitor = RebaseUrls {
        from,
        to,
        fragment_base: Some(fragment_base),
        attribute: false,
        href: false,
    };
    scan(input, &mut [&mut visitor])
}

/// Rewrites local URLs whose source resources are emitted at another path.
pub fn rewrite_urls(
    input: &str, base: &str, mappings: &HashMap<String, String>,
) -> Option<String> {
    if mappings.is_empty() {
        return None;
    }
    let mut visitor = RewriteUrls {
        base,
        mappings,
        attribute: false,
    };
    scan(input, &mut [&mut visitor])
}

/// Returns the local URL paths referenced by a page's rendered HTML.
pub fn local_targets(input: &str, base: &str) -> HashSet<String> {
    let mut visitor = LocalTargets {
        base,
        attribute: false,
        targets: HashSet::new(),
    };
    scan(input, &mut [&mut visitor]);
    visitor.targets
}

/// Returns whether a byte is HTML whitespace.
fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

/// Advances an offset past HTML whitespace.
fn skip_whitespace(bytes: &[u8], offset: &mut usize) {
    while bytes.get(*offset).is_some_and(|byte| is_whitespace(*byte)) {
        *offset += 1;
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use html5gum::emitters::callback::CallbackEvent;
    use html5gum::Span;

    use super::{
        rebase_urls, rebase_urls_with_fragment_base, rewrite_urls, scan,
        Editor, Visitor,
    };
    use std::collections::HashMap;

    #[derive(Default)]
    struct RemoveDataAttribute;

    #[derive(Default)]
    struct ReplaceElement {
        start: Option<usize>,
    }

    impl Visitor for RemoveDataAttribute {
        fn visit(
            &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
            editor: &mut Editor<'_>,
        ) {
            if let CallbackEvent::AttributeName { name } = event
                && *name == b"data-remove"
            {
                editor.remove_attribute(name, span);
            }
        }
    }

    impl Visitor for ReplaceElement {
        fn visit(
            &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
            editor: &mut Editor<'_>,
        ) {
            match event {
                CallbackEvent::OpenStartTag { name } if *name == b"replace" => {
                    self.start = Some(span.start);
                }
                CallbackEvent::EndTag { name } if *name == b"replace" => {
                    let start = self.start.take().expect("start tag");
                    editor.replace(start..span.end, "slot");
                }
                _ => {}
            }
        }
    }

    fn remove(input: &str) -> Option<String> {
        let mut visitor = RemoveDataAttribute;
        scan(input, &mut [&mut visitor])
    }

    #[test]
    fn retains_the_original_allocation_without_edits() {
        assert_eq!(remove("<p>Text</p>"), None);
    }

    #[test]
    fn removes_boolean_and_quoted_attributes() {
        let input = concat!(
            r#"<div data-remove class="one">A</div>"#,
            r#"<div class="two" data-remove = 'yes'>B</div>"#,
        );
        assert_eq!(
            remove(input).as_deref(),
            Some(r#"<div class="one">A</div><div class="two">B</div>"#)
        );
    }

    #[test]
    fn removes_multiline_and_unquoted_attributes() {
        let input = "<div\n  data-remove=value\n  class=x>Text</div>";
        assert_eq!(
            remove(input).as_deref(),
            Some("<div\n  class=x>Text</div>")
        );
    }

    #[test]
    fn outer_replacements_own_contained_attribute_edits() {
        let input = "<replace data-remove>Text</replace>";
        let mut remove = RemoveDataAttribute;
        let mut replace = ReplaceElement::default();

        assert_eq!(
            scan(input, &mut [&mut remove, &mut replace]).as_deref(),
            Some("slot")
        );
    }

    #[test]
    fn renders_nested_edits_before_replacing_the_enclosing_element() {
        let input = "<replace><span data-remove>Label</span></replace>";
        let mut remove = RemoveDataAttribute;
        let mut replace = ReplaceElement::default();
        let inner =
            input.find("<span").unwrap()..input.find("</replace>").unwrap();

        let mut editor = Editor::scan(input, &mut [&mut remove, &mut replace]);
        let title = editor.render_range(inner);
        let output = editor.finish();

        // A reference renderer can retain the edited title even though its
        // enclosing element is replaced with a slot in the shared output.
        assert_eq!(title.as_deref(), Some("<span>Label</span>"));
        assert_eq!(output.as_deref(), Some("slot"));
    }

    #[test]
    fn rebases_link_and_media_attributes_without_reserializing_html() {
        let input = concat!(
            r#"<a href="../../../../notes/#detail">Notes</a>"#,
            r#"<img src='asset.png'>"#,
            r#"<a href="https://example.com">External</a>"#,
        );
        assert_eq!(
            rebase_urls(input, "blog/2026/09/post/", "blog/page/2/").as_deref(),
            Some(concat!(
                r#"<a href="../../../notes/#detail">Notes</a>"#,
                r#"<img src='../../2026/09/post/asset.png'>"#,
                r#"<a href="https://example.com">External</a>"#,
            ))
        );
    }

    #[test]
    fn rebases_excerpt_fragment_links_to_the_full_post() {
        let input = concat!(
            r##"<a href="#detail">Detail</a>"##,
            r#"<img src="asset.png">"#,
        );
        assert_eq!(
            rebase_urls_with_fragment_base(
                input,
                "blog/2026/09/post/",
                "blog/page/2/",
                "../../2026/09/post/",
            )
            .as_deref(),
            Some(concat!(
                r##"<a href="../../2026/09/post/#detail">Detail</a>"##,
                r#"<img src="../../2026/09/post/asset.png">"#,
            ))
        );
    }

    #[test]
    fn rewrites_relocated_resource_urls_without_reserializing_html() {
        let input = concat!(
            r#"<a href="../../../../blog/posts/assets/file.pdf?raw#page">File</a>"#,
            r#"<img src='../../../../blog/posts/assets/image.png'>"#,
            r#"<a href="https://example.com">External</a>"#,
        );
        let mappings = HashMap::from([
            (
                "blog/posts/assets/file.pdf".into(),
                "blog/assets/file.pdf".into(),
            ),
            (
                "blog/posts/assets/image.png".into(),
                "blog/assets/image.png".into(),
            ),
        ]);
        assert_eq!(
            rewrite_urls(input, "blog/2026/09/post/", &mappings).as_deref(),
            Some(concat!(
                r#"<a href="../../../assets/file.pdf?raw#page">File</a>"#,
                r#"<img src='../../../assets/image.png'>"#,
                r#"<a href="https://example.com">External</a>"#,
            ))
        );
    }
}
