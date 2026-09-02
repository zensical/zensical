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

//! Inline script and style editing.

use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use std::ops::Range;

use crate::compat::mkdocs::plugin::minify::{script, style};

use super::syntax::{
    is_css, is_javascript, is_module, source_name, Attribute, InlineKind,
    StartTag, Value,
};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Collects size-reducing replacements for inline scripts and styles.
pub struct InlineEditor<'a> {
    /// Original rendered HTML.
    input: &'a str,
    /// Whether inline JavaScript is minified.
    inline_script: bool,
    /// Whether inline CSS is minified.
    inline_style: bool,
    /// Start tag currently being assembled.
    tag: Option<StartTag>,
    /// Active inline body and its source start.
    active: Option<(InlineKind, usize)>,
    /// Non-overlapping replacements collected in source order.
    edits: Vec<(Range<usize>, String)>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> InlineEditor<'a> {
    /// Creates an inline editor.
    pub fn new(
        input: &'a str, inline_script: bool, inline_style: bool,
    ) -> Self {
        Self {
            input,
            inline_script,
            inline_style,
            tag: None,
            active: None,
            edits: Vec::new(),
        }
    }

    /// Consumes one tokenizer event.
    pub fn event(&mut self, event: CallbackEvent<'_>, span: Span<usize>) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.tag = Some(StartTag {
                    name: String::from_utf8_lossy(name).into_owned(),
                    output_name: source_name(self.input, span, 1, name.len()),
                    attributes: Vec::new(),
                    attribute: None,
                });
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute_name(name, span);
            }
            CallbackEvent::AttributeValue { value } => {
                self.attribute_value(value, span);
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                self.close_start_tag(self_closing, span.end);
            }
            CallbackEvent::EndTag { name } => self.end_tag(name, span.start),
            _ => {}
        }
    }

    fn attribute_name(&mut self, name: &[u8], span: Span<usize>) {
        let Some(tag) = self.tag.as_mut() else {
            return;
        };
        if let Some(attribute) = tag.attribute.take() {
            tag.attributes.push(attribute);
        }
        tag.attribute = Some(Attribute {
            name: String::from_utf8_lossy(name).into_owned(),
            output_name: source_name(self.input, span, 0, name.len()),
            value: None,
        });
    }

    fn attribute_value(&mut self, value: &[u8], span: Span<usize>) {
        if let Some(attribute) =
            self.tag.as_mut().and_then(|tag| tag.attribute.as_mut())
        {
            attribute.value = Some(Value {
                decoded: String::from_utf8_lossy(value).into_owned(),
                raw: self.input[span.start..span.end].into(),
            });
        }
    }

    fn close_start_tag(&mut self, self_closing: bool, end: usize) {
        let Some(mut tag) = self.tag.take() else {
            return;
        };
        if let Some(attribute) = tag.attribute.take() {
            tag.attributes.push(attribute);
        }
        if self_closing {
            return;
        }

        // Record the body start only for enabled, supported inline languages.
        // Unsupported types are left entirely untouched.
        self.active = match tag.name.as_str() {
            "script" if self.inline_script && is_javascript(&tag) => {
                Some((InlineKind::Script { module: is_module(&tag) }, end))
            }
            "style" if self.inline_style && is_css(&tag) => {
                Some((InlineKind::Style, end))
            }
            _ => None,
        };
    }

    fn end_tag(&mut self, name: &[u8], end: usize) {
        // Ignore unrelated end tags until the active inline body closes.
        let expected = match self.active.as_ref().map(|item| item.0) {
            Some(InlineKind::Script { .. }) => b"script".as_slice(),
            Some(InlineKind::Style) => b"style".as_slice(),
            None => return,
        };
        if name != expected {
            return;
        }

        let (kind, start) = self.active.take().expect("active");
        let source = &self.input[start..end];
        let output = match kind {
            InlineKind::Script { module } => script::minify(source, module),
            InlineKind::Style => style::minify(source),
        };

        // Applying only shrinking edits guarantees that inline-only mode never
        // expands the surrounding document.
        if let Some(output) = output
            && output.len() < source.len()
        {
            self.edits.push((start..end, output));
        }
    }

    /// Returns the collected replacements.
    pub fn finish(self) -> Vec<(Range<usize>, String)> {
        self.edits
    }
}
