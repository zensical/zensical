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

//! Streaming HTML serializer.

use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;

use crate::compat::mkdocs::plugin::minify::{script, style};
use crate::config::plugins::HtmlMinOptions;

use super::syntax::{
    closes_on_start, collapse_whitespace, escape_text, is_boolean_attribute,
    is_css, is_html_whitespace, is_javascript, is_module, is_void,
    serialize_attribute_value, source_name, Attribute, Element, Inline,
    InlineKind, StartTag, Value,
};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Stateful serializer consuming html5gum callback events.
pub struct Serializer<'a> {
    /// Original rendered HTML.
    input: &'a str,
    /// HTML minification options.
    options: &'a HtmlMinOptions,
    /// Whether inline JavaScript is minified.
    inline_script: bool,
    /// Whether inline CSS is minified.
    inline_style: bool,
    /// Serialized output.
    output: String,
    /// Start tag currently being assembled.
    start_tag: Option<StartTag>,
    /// Open elements relevant to whitespace and language state.
    elements: Vec<Element>,
    /// Inline body currently being buffered.
    inline: Option<Inline>,
    /// Whether the last emitted token was a doctype.
    after_doctype: bool,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Serializer<'a> {
    /// Creates a serializer.
    pub fn new(
        input: &'a str, options: &'a HtmlMinOptions, inline_script: bool,
        inline_style: bool,
    ) -> Self {
        Self {
            input,
            options,
            inline_script,
            inline_style,
            output: String::with_capacity(input.len()),
            start_tag: None,
            elements: Vec::new(),
            inline: None,
            after_doctype: false,
        }
    }

    /// Consumes one tokenizer event.
    pub fn event(&mut self, event: CallbackEvent<'_>, span: Span<usize>) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.open_start_tag(name, span);
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute_name(name, span);
            }
            CallbackEvent::AttributeValue { value } => {
                self.attribute_value(value, span);
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                self.close_start_tag(self_closing);
            }
            CallbackEvent::EndTag { name } => self.end_tag(name, span),
            CallbackEvent::String { value } => self.text(value, span),
            CallbackEvent::Comment { value } => self.comment(value),
            CallbackEvent::Doctype {
                name,
                public_identifier,
                system_identifier,
                ..
            } => self.doctype(name, public_identifier, system_identifier),
            CallbackEvent::Error(_) => {}
        }
    }

    fn open_start_tag(&mut self, name: &[u8], span: Span<usize>) {
        let name = String::from_utf8_lossy(name).into_owned();
        self.close_optional_element(&name);
        self.start_tag = Some(StartTag {
            output_name: source_name(self.input, span, 1, name.len()),
            name,
            attributes: Vec::new(),
            attribute: None,
        });
        self.after_doctype = false;
    }

    fn attribute_name(&mut self, name: &[u8], span: Span<usize>) {
        let Some(tag) = self.start_tag.as_mut() else {
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
        let Some(attribute) = self
            .start_tag
            .as_mut()
            .and_then(|tag| tag.attribute.as_mut())
        else {
            return;
        };
        attribute.value = Some(Value {
            decoded: String::from_utf8_lossy(value).into_owned(),
            raw: self.input[span.start..span.end].into(),
        });
    }

    fn close_start_tag(&mut self, self_closing: bool) {
        let Some(mut tag) = self.start_tag.take() else {
            return;
        };
        if let Some(attribute) = tag.attribute.take() {
            tag.attributes.push(attribute);
        }

        // Resolve the effective language before serialization can remove a
        // redundant lang attribute.
        let parent_language = self
            .elements
            .last()
            .and_then(|element| element.language.clone());
        let language = tag
            .attributes
            .iter()
            .find(|attribute| attribute.name == "lang")
            .and_then(|attribute| attribute.value.as_ref())
            .map(|value| value.decoded.clone())
            .or_else(|| parent_language.clone());
        let preserve = self.serialize_start_tag(
            &tag,
            self_closing,
            parent_language.as_deref(),
        );

        // Void and explicitly self-closing tags do not affect descendant
        // whitespace or language state.
        if is_void(&tag.name) || self_closing {
            return;
        }

        // Preservation is inherited. Script and style bodies are always kept
        // verbatim until their optional language minifier accepts them.
        let preserve = preserve
            || self.is_preserved()
            || tag.name == "script"
            || tag.name == "style"
            || self
                .options
                .pre_tags
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&tag.name));
        self.elements.push(Element {
            name: tag.name.clone(),
            preserve,
            language,
        });

        // Buffer supported inline languages so parse failures can fall back to
        // the exact original body.
        let kind = match tag.name.as_str() {
            "script" if self.inline_script && is_javascript(&tag) => {
                Some(InlineKind::Script { module: is_module(&tag) })
            }
            "style" if self.inline_style && is_css(&tag) => {
                Some(InlineKind::Style)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            self.inline = Some(Inline { kind, source: String::new() });
        }
    }

    fn serialize_start_tag(
        &mut self, tag: &StartTag, self_closing: bool,
        parent_language: Option<&str>,
    ) -> bool {
        self.output.push('<');
        self.output.push_str(&tag.output_name);

        let mut preserve = false;
        let mut unquoted = false;
        for attribute in &tag.attributes {
            // A configured prefix protects an attribute from normalization;
            // the prefix itself is omitted from rendered output.
            let prefixed = attribute
                .name
                .strip_prefix(&format!("{}-", self.options.pre_attr));
            let name = prefixed.unwrap_or(&attribute.name);
            let protected = prefixed.is_some();
            let output_name = if protected {
                attribute
                    .output_name
                    .get(self.options.pre_attr.len() + 1..)
                    .unwrap_or(&attribute.output_name)
            } else {
                &attribute.output_name
            };

            if name == self.options.pre_attr {
                preserve = true;
                if !self.options.keep_pre && !protected {
                    continue;
                }
            }

            // An inherited language need not be repeated on the child.
            if name == "lang"
                && attribute.value.as_ref().is_some_and(|value| {
                    parent_language == Some(value.decoded.as_str())
                })
            {
                continue;
            }

            self.output.push(' ');
            self.output.push_str(output_name);
            let Some(value) = &attribute.value else {
                unquoted = false;
                continue;
            };

            // Empty and boolean values can be represented by the name alone.
            if (self.options.reduce_empty_attributes
                && value.decoded.is_empty())
                || (self.options.reduce_boolean_attributes
                    && is_boolean_attribute(&tag.name, name))
            {
                unquoted = false;
                continue;
            }

            self.output.push('=');
            // Protected values and disabled character-reference conversion use
            // their source spelling; all other values use decoded text.
            let value = if protected || !self.options.convert_charrefs {
                value.raw.as_str()
            } else {
                value.decoded.as_str()
            };
            unquoted = serialize_attribute_value(
                &mut self.output,
                value,
                self.options.remove_optional_attribute_quotes,
                protected || !self.options.convert_charrefs,
            );
        }

        if self_closing && !is_void(&tag.name) {
            // Without whitespace, the solidus is part of an unquoted value,
            // so the HTML parser does not acknowledge the self-closing flag.
            if unquoted {
                self.output.push(' ');
            }
            self.output.push_str("/>");
        } else {
            self.output.push('>');
        }
        preserve
    }

    fn end_tag(&mut self, name: &[u8], span: Span<usize>) {
        let name = String::from_utf8_lossy(name);

        // Flush a buffered language body before emitting its closing tag.
        if matches!(name.as_ref(), "script" | "style") {
            self.finish_inline();
        }

        // htmlmin removes trailing whitespace from titles specifically.
        if name == "title" {
            while self.output.ends_with(char::is_whitespace) {
                self.output.pop();
            }
        }
        if !is_void(&name) {
            self.output.push_str("</");
            self.output
                .push_str(&source_name(self.input, span, 2, name.len()));
            self.output.push('>');
        }

        // Search backwards to recover cleanly from imperfectly nested input.
        if let Some(index) = self
            .elements
            .iter()
            .rposition(|element| element.name == name)
        {
            self.elements.truncate(index);
        }
    }

    fn text(&mut self, value: &[u8], span: Span<usize>) {
        // Buffered and preserved contexts retain source bytes, bypassing HTML
        // whitespace and entity normalization.
        if let Some(inline) = self.inline.as_mut() {
            inline.source.push_str(&self.input[span.start..span.end]);
            return;
        }
        if self.is_preserved() {
            self.output.push_str(&self.input[span.start..span.end]);
            return;
        }

        let value = String::from_utf8_lossy(value);
        let only_whitespace = value.chars().all(is_html_whitespace);

        // The three whitespace options differ only in where an all-whitespace
        // token may be dropped completely.
        if only_whitespace
            && (self.options.remove_all_empty_space
                || self.in_head()
                || self.after_doctype
                || (self.options.remove_empty_space
                    && value.contains(['\n', '\r'])))
        {
            return;
        }

        let mut value = collapse_whitespace(&value);

        // Trim boundaries that would otherwise survive token-by-token
        // processing and produce duplicate spaces.
        if self.in_title() && self.output.ends_with("<title>") {
            value = value.trim_start().into();
        }
        if self.output.ends_with(' ') && value.starts_with(' ') {
            value.remove(0);
        }
        escape_text(&mut self.output, &value);
    }

    fn comment(&mut self, value: &[u8]) {
        let value = String::from_utf8_lossy(value);
        if self.options.remove_comments {
            // Legal comments and conditional comments remain observable even
            // when ordinary comments are removed.
            if let Some(value) = value.strip_prefix('!') {
                self.output.push_str("<!--");
                self.output.push_str(value);
                self.output.push_str("-->");
            } else if value.trim_start().starts_with("[if ") {
                self.output.push_str("<!--");
                self.output.push_str(&value);
                self.output.push_str("-->");
            }
        } else {
            self.output.push_str("<!--");
            self.output.push_str(&value);
            self.output.push_str("-->");
        }
    }

    fn doctype(
        &mut self, name: &[u8], public: Option<&[u8]>, system: Option<&[u8]>,
    ) {
        self.output.push_str("<!doctype ");
        self.output.push_str(&String::from_utf8_lossy(name));
        if let Some(public) = public {
            self.output.push_str(" public \"");
            self.output.push_str(&String::from_utf8_lossy(public));
            self.output.push('"');
        }
        if let Some(system) = system {
            if public.is_none() {
                self.output.push_str(" system");
            }
            self.output.push_str(" \"");
            self.output.push_str(&String::from_utf8_lossy(system));
            self.output.push('"');
        }
        self.output.push('>');
        self.after_doctype = true;
    }

    fn finish_inline(&mut self) {
        let Some(inline) = self.inline.take() else {
            return;
        };
        let output = match inline.kind {
            InlineKind::Script { module } => {
                script::minify(&inline.source, module)
            }
            InlineKind::Style => style::minify(&inline.source),
        };

        // A minifier may reject valid-but-unsupported syntax or produce larger
        // output. In either case, retain the original body byte-for-byte.
        if let Some(output) =
            output.filter(|output| output.len() < inline.source.len())
        {
            self.output.push_str(&output);
        } else {
            self.output.push_str(&inline.source);
        }
    }

    fn is_preserved(&self) -> bool {
        self.elements.last().is_some_and(|element| element.preserve)
    }

    fn in_head(&self) -> bool {
        self.elements.iter().any(|element| element.name == "head")
    }

    fn in_title(&self) -> bool {
        self.elements
            .last()
            .is_some_and(|element| element.name == "title")
    }

    fn close_optional_element(&mut self, next: &str) {
        let Some(current) = self.elements.last() else {
            return;
        };
        if closes_on_start(&current.name, next) {
            self.elements.pop();
        }
    }

    /// Finishes buffered inline content and returns the output.
    pub fn finish(mut self) -> String {
        self.finish_inline();
        self.output
    }
}
