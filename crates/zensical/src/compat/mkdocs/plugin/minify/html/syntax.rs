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

//! HTML syntax facts and serialization helpers.

use html5gum::Span;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Inline language selected from a script or style element.
#[derive(Clone, Copy, Debug)]
pub enum InlineKind {
    /// JavaScript, optionally parsed as a module.
    Script { module: bool },
    /// CSS.
    Style,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One parsed attribute waiting for serialization.
#[derive(Debug)]
pub struct Attribute {
    /// Normalized attribute name.
    pub name: String,
    /// Attribute name as it appeared in the source.
    pub output_name: String,
    /// Optional attribute value.
    pub value: Option<Value>,
}

/// One parsed attribute value in decoded and source forms.
#[derive(Debug)]
pub struct Value {
    /// Decoded attribute value.
    pub decoded: String,
    /// Attribute value as it appeared in the source.
    pub raw: String,
}

/// One start tag being assembled from tokenizer events.
#[derive(Debug)]
pub struct StartTag {
    /// Normalized element name.
    pub name: String,
    /// Name as it appeared in the source.
    pub output_name: String,
    /// Completed attributes.
    pub attributes: Vec<Attribute>,
    /// Attribute currently being assembled.
    pub attribute: Option<Attribute>,
}

/// One open element relevant to minification state.
#[derive(Debug)]
pub struct Element {
    /// Normalized element name.
    pub name: String,
    /// Whether descendant text must be preserved verbatim.
    pub preserve: bool,
    /// Effective inherited language.
    pub language: Option<String>,
}

/// Inline language content buffered until its closing tag.
#[derive(Debug)]
pub struct Inline {
    /// Inline language kind.
    pub kind: InlineKind,
    /// Buffered inline source.
    pub source: String,
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Collapses consecutive HTML whitespace.
pub fn collapse_whitespace(value: &str) -> String {
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

/// Returns an element or attribute name with its source casing.
pub fn source_name(
    input: &str, span: Span<usize>, prefix: usize, length: usize,
) -> String {
    let start = span.start.saturating_add(prefix);
    let end = (start + length).min(input.len());
    input.get(start..end).unwrap_or_default().into()
}

/// Returns whether a character is HTML whitespace.
pub fn is_html_whitespace(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\u{c}' | '\r' | ' ')
}

/// Escapes text for serialization.
pub fn escape_text(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            _ => output.push(character),
        }
    }
}

/// Serializes an attribute value with minimal safe quoting.
pub fn serialize_attribute_value(
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

/// Escapes ampersands in an attribute value.
fn escape_ampersands(output: &mut String, value: &str) {
    for character in value.chars() {
        if character == '&' {
            output.push_str("&amp;");
        } else {
            output.push(character);
        }
    }
}

/// Returns whether a character is allowed in an unquoted value.
fn is_unquoted(character: char) -> bool {
    !is_html_whitespace(character)
        && !matches!(character, '"' | '\'' | '`' | '=' | '<' | '>')
}

/// Returns whether an element is void.
pub fn is_void(name: &str) -> bool {
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

/// Returns whether an attribute is boolean for an element.
pub fn is_boolean_attribute(tag: &str, name: &str) -> bool {
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

/// Returns whether a start tag implicitly closes the current element.
pub fn closes_on_start(current: &str, next: &str) -> bool {
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

/// Returns whether a script tag contains JavaScript.
pub fn is_javascript(tag: &StartTag) -> bool {
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

/// Returns whether a style tag contains CSS.
pub fn is_css(tag: &StartTag) -> bool {
    attribute(tag, "type").is_none_or(|kind| {
        let kind = kind.trim().to_ascii_lowercase();
        kind.is_empty()
            || kind
                .split(';')
                .next()
                .is_some_and(|kind| kind.trim() == "text/css")
    })
}

/// Returns whether a script tag is a module.
pub fn is_module(tag: &StartTag) -> bool {
    attribute(tag, "type")
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("module"))
}

/// Returns a decoded attribute value by name.
fn attribute<'a>(tag: &'a StartTag, name: &str) -> Option<&'a str> {
    tag.attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .and_then(|attribute| attribute.value.as_ref())
        .map(|value| value.decoded.as_str())
}
