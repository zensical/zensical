// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Shared HTML processing for MkDocs-compatible plugins.

use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Span, Tokenizer};
use std::convert::Infallible;
use std::ops::Range;

// ----------------------------------------------------------------------------
// Traits
// ----------------------------------------------------------------------------

/// Page-local observer participating in the shared HTML pass.
pub(crate) trait Visitor {
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
pub(crate) struct Editor<'a> {
    /// Original HTML input.
    input: &'a str,
    /// Edits recorded by visitors.
    edits: Vec<Edit>,
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
    /// Creates an editor for an HTML input.
    fn new(input: &'a str) -> Self {
        Self { input, edits: Vec::new() }
    }

    /// Returns original HTML covered by a tokenizer span.
    pub(crate) fn text(&self, range: Range<usize>) -> &str {
        &self.input[range]
    }

    /// Replaces a byte range after all visitors have observed the input.
    pub(crate) fn replace(
        &mut self, range: Range<usize>, replacement: impl Into<Box<str>>,
    ) {
        assert!(range.start <= range.end && range.end <= self.input.len());
        self.edits.push(Edit {
            range,
            replacement: replacement.into(),
        });
    }

    /// Removes the complete attribute whose name occupies `span`.
    pub(crate) fn remove_attribute(&mut self, name: &[u8], span: Span<usize>) {
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

    /// Applies all deferred edits in one linear output pass.
    fn finish(mut self) -> Option<String> {
        if self.edits.is_empty() {
            return None;
        }

        // Outer edits sort before edits they contain. An outer replacement
        // owns its complete input span, while partially overlapping edits are
        // always a programming error between visitors.
        self.edits.sort_by(|left, right| {
            left.range
                .start
                .cmp(&right.range.start)
                .then_with(|| right.range.end.cmp(&left.range.end))
        });

        let mut edits: Vec<Edit> = Vec::with_capacity(self.edits.len());
        for edit in self.edits {
            if let Some(previous) = edits.last() {
                if edit.range.start < previous.range.end {
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
            }
            edits.push(edit);
        }

        let removed = edits.iter().map(|edit| edit.range.len()).sum::<usize>();
        let inserted = edits
            .iter()
            .map(|edit| edit.replacement.len())
            .sum::<usize>();
        let mut output = String::with_capacity(
            self.input.len().saturating_sub(removed) + inserted,
        );
        let mut cursor = 0;
        for edit in edits {
            assert!(self.input.is_char_boundary(edit.range.start));
            assert!(self.input.is_char_boundary(edit.range.end));
            output.push_str(&self.input[cursor..edit.range.start]);
            output.push_str(&edit.replacement);
            cursor = edit.range.end;
        }
        output.push_str(&self.input[cursor..]);
        Some(output)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Scans HTML once with all page-local visitors.
///
/// Returns modified HTML only when a visitor recorded an edit, allowing the
/// caller to retain the original allocation for observational passes.
pub(crate) fn scan(
    input: &str, visitors: &mut [&mut dyn Visitor],
) -> Option<String> {
    let mut editor = Editor::new(input);
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
    editor.finish()
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
    use super::*;

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
}
