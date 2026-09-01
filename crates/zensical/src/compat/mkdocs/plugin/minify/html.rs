// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! HTML minification over the shared HTML tokenizer.

use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Span, Tokenizer};
use std::convert::Infallible;
use std::ops::Range;

use crate::config::plugins::HtmlMinOptions;

use super::{script, style};

/// Minifies a complete rendered HTML document.
pub(super) fn minify(
    input: &str, options: &HtmlMinOptions, inline_script: bool,
    inline_style: bool,
) -> String {
    let mut serializer =
        Serializer::new(input, options, inline_script, inline_style);
    {
        let mut emitter = CallbackEmitter::new(
            |event: CallbackEvent<'_>, span: Span<usize>| {
                serializer.event(event, span);
                None::<Infallible>
            },
        );
        emitter.naively_switch_states(true);
        Tokenizer::new_with_emitter(input, emitter)
            .finish()
            .expect("string input is infallible");
    }
    serializer.finish()
}

/// Minifies only inline language bodies, retaining all surrounding HTML.
pub(super) fn minify_inline(
    input: String, inline_script: bool, inline_style: bool,
) -> String {
    let edits = {
        let mut editor = InlineEditor::new(&input, inline_script, inline_style);
        {
            let mut emitter = CallbackEmitter::new(
                |event: CallbackEvent<'_>, span: Span<usize>| {
                    editor.event(event, span);
                    None::<Infallible>
                },
            );
            emitter.naively_switch_states(true);
            Tokenizer::new_with_emitter(input.as_str(), emitter)
                .finish()
                .expect("string input is infallible");
        }
        editor.finish()
    };
    if edits.is_empty() {
        return input;
    }

    let removed = edits.iter().map(|(range, _)| range.len()).sum::<usize>();
    let inserted = edits.iter().map(|(_, value)| value.len()).sum::<usize>();
    let mut output = String::with_capacity(input.len() - removed + inserted);
    let mut cursor = 0;
    for (range, value) in edits {
        output.push_str(&input[cursor..range.start]);
        output.push_str(&value);
        cursor = range.end;
    }
    output.push_str(&input[cursor..]);
    output
}

/// Span editor used when only inline language minification is enabled.
struct InlineEditor<'a> {
    input: &'a str,
    inline_script: bool,
    inline_style: bool,
    tag: Option<StartTag>,
    active: Option<(InlineKind, usize)>,
    edits: Vec<(Range<usize>, String)>,
}

impl<'a> InlineEditor<'a> {
    fn new(input: &'a str, inline_script: bool, inline_style: bool) -> Self {
        Self {
            input,
            inline_script,
            inline_style,
            tag: None,
            active: None,
            edits: Vec::new(),
        }
    }

    fn event(&mut self, event: CallbackEvent<'_>, span: Span<usize>) {
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
        if let Some(output) = output
            && output.len() < source.len()
        {
            self.edits.push((start..end, output));
        }
    }

    fn finish(self) -> Vec<(Range<usize>, String)> {
        self.edits
    }
}

/// One parsed attribute waiting for serialization.
#[derive(Debug)]
struct Attribute {
    name: String,
    output_name: String,
    value: Option<Value>,
}

/// One parsed attribute value in decoded and source forms.
#[derive(Debug)]
struct Value {
    decoded: String,
    raw: String,
}

/// One start tag being assembled from tokenizer events.
#[derive(Debug)]
struct StartTag {
    name: String,
    output_name: String,
    attributes: Vec<Attribute>,
    attribute: Option<Attribute>,
}

/// One open element relevant to minification state.
#[derive(Debug)]
struct Element {
    name: String,
    preserve: bool,
    language: Option<String>,
}

/// Inline language content buffered until its closing tag.
#[derive(Debug)]
struct Inline {
    kind: InlineKind,
    source: String,
}

/// Inline language selected from a script or style element.
#[derive(Clone, Copy, Debug)]
enum InlineKind {
    Script { module: bool },
    Style,
}

/// Stateful serializer consuming html5gum callback events.
struct Serializer<'a> {
    input: &'a str,
    options: &'a HtmlMinOptions,
    inline_script: bool,
    inline_style: bool,
    output: String,
    start_tag: Option<StartTag>,
    elements: Vec<Element>,
    inline: Option<Inline>,
    after_doctype: bool,
}

impl<'a> Serializer<'a> {
    fn new(
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

    fn event(&mut self, event: CallbackEvent<'_>, span: Span<usize>) {
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

        if is_void(&tag.name) || self_closing {
            return;
        }

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
        for attribute in &tag.attributes {
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
                continue;
            };
            if (self.options.reduce_empty_attributes
                && value.decoded.is_empty())
                || (self.options.reduce_boolean_attributes
                    && is_boolean_attribute(&tag.name, name))
            {
                continue;
            }

            self.output.push('=');
            let value = if protected || !self.options.convert_charrefs {
                value.raw.as_str()
            } else {
                value.decoded.as_str()
            };
            serialize_attribute_value(
                &mut self.output,
                value,
                self.options.remove_optional_attribute_quotes,
                protected || !self.options.convert_charrefs,
            );
        }

        if self_closing && !is_void(&tag.name) {
            self.output.push_str("/>");
        } else {
            self.output.push('>');
        }
        preserve
    }

    fn end_tag(&mut self, name: &[u8], span: Span<usize>) {
        let name = String::from_utf8_lossy(name);
        if matches!(name.as_ref(), "script" | "style") {
            self.finish_inline();
        }
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
        if let Some(index) = self
            .elements
            .iter()
            .rposition(|element| element.name == name)
        {
            self.elements.truncate(index);
        }
    }

    fn text(&mut self, value: &[u8], span: Span<usize>) {
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

    fn finish(mut self) -> String {
        self.finish_inline();
        self.output
    }
}

fn collapse_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut whitespace = false;
    for character in value.chars() {
        if is_html_whitespace(character) {
            whitespace = true;
        } else {
            if whitespace {
                output.push(' ');
                whitespace = false;
            }
            output.push(character);
        }
    }
    if whitespace {
        output.push(' ');
    }
    output
}

fn source_name(
    input: &str, span: Span<usize>, prefix: usize, length: usize,
) -> String {
    let start = span.start.saturating_add(prefix);
    let end = (start + length).min(input.len());
    input.get(start..end).unwrap_or_default().into()
}

fn is_html_whitespace(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\u{c}' | '\r' | ' ')
}

fn escape_text(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            _ => output.push(character),
        }
    }
}

fn serialize_attribute_value(
    output: &mut String, value: &str, remove_quotes: bool,
    preserve_references: bool,
) {
    if remove_quotes && !value.is_empty() && value.chars().all(is_unquoted) {
        if preserve_references {
            output.push_str(value);
        } else {
            escape_ampersands(output, value);
        }
        return;
    }

    let single = value.matches('\'').count();
    let double = value.matches('"').count();
    let quote = if double > single { '\'' } else { '"' };
    output.push(quote);
    for character in value.chars() {
        match character {
            '&' if !preserve_references => output.push_str("&amp;"),
            '\'' if quote == '\'' => output.push_str("&#39;"),
            '"' if quote == '"' => output.push_str("&#34;"),
            _ => output.push(character),
        }
    }
    output.push(quote);
}

fn escape_ampersands(output: &mut String, value: &str) {
    for character in value.chars() {
        if character == '&' {
            output.push_str("&amp;");
        } else {
            output.push(character);
        }
    }
}

fn is_unquoted(character: char) -> bool {
    !is_html_whitespace(character)
        && !matches!(character, '"' | '\'' | '`' | '=' | '<' | '>')
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "command"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_boolean_attribute(tag: &str, name: &str) -> bool {
    if name == "hidden" {
        return true;
    }
    match tag {
        "audio" | "video" => {
            matches!(name, "autoplay" | "controls" | "loop" | "muted")
        }
        "button" => matches!(name, "autofocus" | "disabled" | "formnovalidate"),
        "dialog" => name == "open",
        "fieldset" | "optgroup" => name == "disabled",
        "form" => name == "novalidate",
        "iframe" => name == "seamless",
        "img" => name == "ismap",
        "input" => matches!(
            name,
            "autofocus"
                | "checked"
                | "disabled"
                | "formnovalidate"
                | "multiple"
                | "readonly"
                | "required"
        ),
        "keygen" => matches!(name, "autofocus" | "disabled"),
        "object" => name == "typemustmatch",
        "ol" => name == "reversed",
        "option" => matches!(name, "disabled" | "selected"),
        "script" => matches!(name, "async" | "defer"),
        "select" => {
            matches!(name, "autofocus" | "disabled" | "multiple" | "required")
        }
        "style" => name == "scoped",
        "textarea" => {
            matches!(name, "autofocus" | "disabled" | "readonly" | "required")
        }
        "track" => name == "default",
        _ => false,
    }
}

fn closes_on_start(current: &str, next: &str) -> bool {
    match current {
        "li" => next == "li",
        "dd" | "dt" => matches!(next, "dd" | "dt"),
        "rp" | "rt" => matches!(next, "rp" | "rt"),
        "p" => matches!(
            next,
            "address"
                | "article"
                | "aside"
                | "blockquote"
                | "dir"
                | "div"
                | "dl"
                | "fieldset"
                | "footer"
                | "form"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "header"
                | "hgroup"
                | "hr"
                | "menu"
                | "nav"
                | "ol"
                | "p"
                | "pre"
                | "section"
                | "table"
                | "ul"
        ),
        "option" => matches!(next, "option" | "optgroup"),
        "optgroup" => next == "optgroup",
        "colgroup" => true,
        "thead" | "tbody" => matches!(next, "tbody" | "tfoot"),
        "tfoot" => next == "tbody",
        "tr" => next == "tr",
        "td" | "th" => matches!(next, "td" | "th"),
        _ => false,
    }
}

fn is_javascript(tag: &StartTag) -> bool {
    let kind = attribute(tag, "type")
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    let language = attribute(tag, "language")
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    match kind.as_deref() {
        None | Some("" | "module") => {}
        Some(kind)
            if matches!(
                kind.split(';').next().map(str::trim),
                Some(
                    "text/javascript"
                        | "application/javascript"
                        | "text/ecmascript"
                        | "application/ecmascript"
                )
            ) => {}
        _ => return false,
    }
    language.as_deref().is_none_or(|language| {
        matches!(language, "javascript" | "ecmascript" | "jscript")
    })
}

fn is_css(tag: &StartTag) -> bool {
    attribute(tag, "type").is_none_or(|kind| {
        let kind = kind.trim().to_ascii_lowercase();
        kind.is_empty()
            || kind
                .split(';')
                .next()
                .is_some_and(|kind| kind.trim() == "text/css")
    })
}

fn is_module(tag: &StartTag) -> bool {
    attribute(tag, "type")
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("module"))
}

fn attribute<'a>(tag: &'a StartTag, name: &str) -> Option<&'a str> {
    tag.attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .and_then(|attribute| attribute.value.as_ref())
        .map(|value| value.decoded.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> HtmlMinOptions {
        HtmlMinOptions {
            remove_comments: false,
            remove_empty_space: false,
            remove_all_empty_space: false,
            reduce_empty_attributes: true,
            reduce_boolean_attributes: false,
            remove_optional_attribute_quotes: true,
            convert_charrefs: true,
            keep_pre: false,
            pre_tags: vec!["pre".into(), "textarea".into()],
            pre_attr: "pre".into(),
        }
    }

    #[test]
    fn matches_default_htmlmin_behavior() {
        let input = r#"<!doctype html>
            <html><head><!-- keep --><title>  A   title  </title></head>
            <body><input disabled="disabled" class="">
            <p> A   <strong> B </strong> </p>
            <pre>  exact
 value </pre><section pre>  exact  </section></body></html>"#;
        let output = minify(input, &options(), false, false);
        assert_eq!(
            output,
            "<!doctype html><html><head><!-- keep --><title>A title</title></head> \
<body><input disabled=disabled class> <p> A <strong> B </strong> </p> \
<pre>  exact\n value </pre><section>  exact  </section></body></html>"
        );
    }

    #[test]
    fn supports_comment_and_whitespace_modes() {
        let mut options = options();
        options.remove_comments = true;
        options.remove_empty_space = true;
        let output = minify(
            "<body>\n<div>A</div> \t <div>B</div><!-- x --><!--! y --></body>",
            &options,
            false,
            false,
        );
        assert_eq!(output, "<body><div>A</div> <div>B</div><!-- y --></body>");

        options.remove_all_empty_space = true;
        let output =
            minify("<body> <i>A</i> <i>B</i> </body>", &options, false, false);
        assert_eq!(output, "<body><i>A</i><i>B</i></body>");
    }

    #[test]
    fn supports_attribute_options() {
        let mut options = options();
        options.reduce_boolean_attributes = true;
        options.remove_optional_attribute_quotes = false;
        options.convert_charrefs = false;
        let output = minify(
            r#"<input disabled="disabled" title="a&amp;b" alt="">"#,
            &options,
            false,
            false,
        );
        assert_eq!(output, r#"<input disabled title="a&amp;b" alt>"#);
    }

    #[test]
    fn supports_custom_preservation_markers() {
        let mut options = options();
        options.pre_attr = "custom".into();
        options.pre_tags.push("code".into());
        let output = minify(
            "<div custom custom-title='a&amp;b'>  exact  </div><code>  code  </code>",
            &options,
            false,
            false,
        );
        assert_eq!(
            output,
            "<div title=a&amp;b>  exact  </div><code>  code  </code>"
        );
    }

    #[test]
    fn supports_preservation_configuration() {
        let mut options = options();
        options.keep_pre = true;
        assert_eq!(
            minify(
                r#"<strong pre style="">  exact  </strong>"#,
                &options,
                false,
                false,
            ),
            "<strong pre style>  exact  </strong>"
        );

        options.pre_tags.clear();
        assert_eq!(
            minify("<pre>  compact  </pre>", &options, false, false),
            "<pre> compact </pre>"
        );
    }

    #[test]
    fn controls_character_reference_conversion() {
        let input =
            r#"<input value="&#34;&#39;&#39;&#39;&lt;&#46;&pi;&gt; &#34;">"#;
        let converted = minify(input, &options(), false, false);
        assert!(converted.contains(".π>"));
        assert!(!converted.contains("&#46;"));

        let mut options = options();
        options.convert_charrefs = false;
        assert_eq!(minify(input, &options, false, false), input);
    }

    #[test]
    fn preserves_scripts_styles_and_conditional_comments_by_default() {
        let mut options = options();
        options.remove_comments = true;
        let input = "<script>  const x = 1;  </script><style>  .a { color: red }  </style><!--[if IE]>x<![endif]-->";
        assert_eq!(minify(input, &options, false, false), input);
    }

    #[test]
    fn preserves_foreign_element_and_attribute_case() {
        let input = r#"<svg viewBox="0 0 10 10"><linearGradient gradientUnits="userSpaceOnUse"></linearGradient></svg>"#;
        assert_eq!(
            minify(input, &options(), false, false),
            r#"<svg viewBox="0 0 10 10"><linearGradient gradientUnits=userSpaceOnUse></linearGradient></svg>"#
        );
    }

    #[test]
    fn minifies_supported_inline_languages() {
        let input = r#"<script> const value = 1 + 2; </script>
<script type="application/ld+json"> { "value": 3 } </script>
<style> @media screen and (min-width: 45em) { .a { color: red; } } </style>"#;
        let output = minify(input, &options(), true, true);
        assert!(output.contains("<script>const value=1+2;</script>"));
        assert!(output.contains(
            "<script type=application/ld+json> { \"value\": 3 } </script>"
        ));
        assert!(output.contains("screen and (min-width:45em)"));
    }

    #[test]
    fn minifies_module_scripts_as_modules() {
        let input = "<script type=module> await Promise.resolve(1); </script>";
        assert_eq!(
            minify(input, &options(), true, false),
            "<script type=module>await Promise.resolve(1);</script>"
        );
    }

    #[test]
    fn inline_only_retains_surrounding_html() {
        let input =
            "<div  class=\"x\">  A  </div><script> const x = 1; </script>";
        let output = minify_inline(input.into(), true, false);
        assert!(output.starts_with("<div  class=\"x\">  A  </div>"));
        assert!(output.ends_with("<script>const x=1;</script>"));
    }

    #[test]
    fn inline_minification_never_expands_a_body() {
        let input = "<script>x()</script><style>a{color:red}</style>";
        let owned = input.to_string();
        let pointer = owned.as_ptr();
        let output = minify_inline(owned, true, true);
        assert_eq!(output, input);
        assert_eq!(output.as_ptr(), pointer);
        assert_eq!(minify(input, &options(), true, true), input);
    }

    #[test]
    fn invalid_inline_languages_are_preserved() {
        let input = "<script> const = ; </script><style>.a { color: ;</style>";
        assert_eq!(minify_inline(input.into(), true, true), input);
    }
}
