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

//! GitHub Flavored Markdown serialization during the shared HTML pass.

use fluent_uri::{Uri, UriRef};
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use percent_encoding::percent_decode_str;
use regex::{Captures, Regex};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::LazyLock;

use crate::compat::mkdocs::html::{Editor, Visitor};

/// Markdown converter retaining only the currently open HTML elements.
pub struct Parser {
    /// Whether to discard decoration and generated controls.
    autoclean: bool,
    /// Absolute URL against which page links are resolved.
    url: String,
    /// Open elements, including an implicit document root.
    stack: Vec<Element>,
    /// Start tag whose attributes are still arriving.
    start: Element,
    /// Attribute currently receiving decoded text.
    attribute: String,
}

/// How an element contributes to its parent.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Markdown,
    Literal,
    Html,
    Skip,
}

/// One element and its already converted children.
#[derive(Default)]
struct Element {
    tag: String,
    attributes: BTreeMap<String, String>,
    body: String,
    mode: Mode,
    language: String,
    number: u64,
    preserve_html_lines: bool,
    html_body: Option<String>,
    table_rows: Vec<Vec<TableCell>>,
    table_cells: Vec<TableCell>,
    table_fallback: bool,
    footnote_reference: bool,
    admonition_title: String,
}

/// One table cell, including the column formatting supplied by HTML.
struct TableCell {
    body: String,
    header: bool,
    alignment: Option<&'static str>,
}

impl Parser {
    /// Creates a converter for one selected page.
    pub fn new(autoclean: bool, url: String) -> Self {
        Self {
            autoclean,
            url,
            stack: vec![Element::default()],
            start: Element::default(),
            attribute: String::new(),
        }
    }

    /// Closes any remaining elements and returns a newline-terminated document.
    pub fn finish(mut self) -> String {
        while self.stack.len() > 1 {
            self.close();
        }
        let body = self.stack.pop().expect("document root").body;
        document(&body)
    }

    fn open(&mut self, mut element: Element, self_closing: bool) {
        let parent = self.stack.last().expect("document root");
        let in_table = parent.html_body.is_some();
        let footnote_decoration = element.has_class("footnote-backref")
            || element.attributes.contains_key("data-footnote-backref")
            || parent.is_footnotes()
                && (element.tag == "hr"
                    || element.attr("id") == "footnote-label");
        element.mode = parent.mode;
        element.preserve_html_lines = parent.preserve_html_lines
            || matches!(element.tag.as_str(), "pre" | "textarea");
        element.language.clone_from(&parent.language);
        if let Some(language) = element
            .attr("class")
            .split_ascii_whitespace()
            .find_map(|class| class.strip_prefix("language-"))
        {
            element.language = language.into();
        }
        if self.autoclean && matches!(element.tag.as_str(), "img" | "svg") {
            // Upstream drops the complete link when its label contains an image.
            if let Some(link) =
                self.stack.iter_mut().rev().find(|item| item.tag == "a")
            {
                link.mode = Mode::Skip;
            }
        }
        if matches!(element.tag.as_str(), "script" | "style" | "head")
            || footnote_decoration
            || element.has_class("mkdocstrings-source")
            || self.autoclean && element.is_decoration()
        {
            element.mode = Mode::Skip;
        } else if element.mode == Mode::Markdown {
            element.mode = match element.tag.as_str() {
                "pre" | "code" => Mode::Literal,
                "table"
                    if !self.autoclean
                        && element.has_class("highlighttable") =>
                {
                    Mode::Html
                }
                "sub" | "sup" | "kbd" | "mark" | "svg" | "math" => Mode::Html,
                _ => Mode::Markdown,
            };
        }
        if in_table
            || element.tag == "table"
                && element.mode == Mode::Markdown
                && !element.has_class("highlighttable")
        {
            // Retain HTML until we know the table fits GFM's cell model.
            element.html_body = Some(String::new());
            element.table_fallback = in_table
                && (element.alert_kind().is_some() || matches!(
                    element.tag.as_str(),
                    "table" | "pre" | "ul" | "ol" | "blockquote"
                        | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                        | "hr" | "col" | "colgroup"
                ))
                || ["rowspan", "colspan"].iter().any(|name| {
                    element.attr(name).parse::<usize>().is_ok_and(|span| span != 1)
                });
        }
        if element.tag == "ol" {
            element.number =
                element.attr("start").parse().unwrap_or(1).min(999_999_999);
        }
        let void = is_void(&element.tag);
        self.stack.push(element);
        if void || self_closing {
            self.close();
        }
    }

    fn close(&mut self) {
        let mut element = self.stack.pop().expect("open element");
        let task_checkbox = element.tag == "input"
            && self.stack.iter().rev()
                .take_while(|item| item.body.trim().is_empty())
                .any(|item| item.tag == "li");
        let alert = element.alert_kind().map(|kind| {
            // GitHub only renders alerts outside lists, quotes and other alerts.
            let nested = self.stack.iter().any(|item| {
                matches!(item.tag.as_str(), "li" | "blockquote")
                    || item.alert_kind().is_some()
            });
            (kind, nested)
        });
        let parent = self.stack.last_mut().expect("document root");
        if element.mode == Mode::Skip {
            return;
        }
        if parent.mode == Mode::Literal && !element.language.is_empty() {
            parent.language.clone_from(&element.language);
        }
        if element.tag == "code" && element.body.is_empty() {
            return;
        }
        let html = element.html_body.as_ref().map(|body| element.html(&self.url, body));
        if let (Some(output), Some(html)) = (&mut parent.html_body, &html) {
            output.push_str(html);
        }
        parent.table_fallback |= element.table_fallback;
        if parent.mode == Mode::Literal {
            parent.body.push_str(&element.body);
            return;
        }
        if element.tag == "a"
            && (element.has_class("footnote-ref")
                || element.attributes.contains_key("data-footnote-ref"))
            && let Some(label) = footnote_label(&self.url, element.attr("href"))
        {
            let _ = write!(parent.body, "[^{label}]");
            parent.footnote_reference = true;
            return;
        }
        if element.tag == "sup" && element.footnote_reference {
            parent.body.push_str(&element.body);
            return;
        }
        if element.mode == Mode::Html {
            let html = element.html(&self.url, &element.body);
            if parent.mode == Mode::Html || !is_block(&element.tag) {
                parent.body.push_str(&html);
            } else {
                block(&mut parent.body, &html);
            }
            return;
        }
        if parent.alert_kind().is_some()
            && (element.tag == "p" && element.has_class("admonition-title")
                || element.tag == "summary" && parent.tag == "details")
        {
            parent.admonition_title.push_str(element.body.trim());
            return;
        }
        if let Some((kind, nested)) = alert {
            block(&mut parent.body, &element.alert(kind, nested));
            return;
        }
        Self::append_markdown(&mut element, parent, &self.url, html.as_deref(), task_checkbox);
    }

    fn append_markdown(
        element: &mut Element, parent: &mut Element, url: &str,
        html: Option<&str>, task_checkbox: bool,
    ) {
        let body = &element.body;
        match element.tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = usize::from(element.tag.as_bytes()[1] - b'0');
                block(
                    &mut parent.body,
                    &format!("{} {}", "#".repeat(level), body.trim()),
                );
            }
            "pre" => {
                let fence = "`".repeat((longest_run(body, '`') + 1).max(3));
                let language = element.language.replace(['`', '\n', '\r'], "");
                let newline = if body.is_empty() || body.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                block(
                    &mut parent.body,
                    &format!("{fence}{language}\n{body}{newline}{fence}"),
                );
            }
            "code" => parent.body.push_str(&code_span(body)),
            "strong" | "b" => parent.body.push_str(&emphasis(body, "**")),
            "em" | "i" => parent.body.push_str(&emphasis(body, "*")),
            "del" | "s" | "strike" => parent.body.push_str(&emphasis(body, "~~")),
            "a" if !element.attr("href").is_empty() => {
                element.link(&mut parent.body, url);
            }
            "img" => {
                let target = absolute(url, element.attr("src"));
                let _ = write!(
                    parent.body,
                    "![{}]({})",
                    escape(element.attr("alt")),
                    destination(&target)
                );
            }
            "br" => {
                trim_spaces(&mut parent.body);
                parent.body.push_str("  \n");
            }
            "hr" => block(&mut parent.body, "---"),
            "blockquote" => block(&mut parent.body, &blockquote(body)),
            "li" => {
                if let Some(label) = footnote_label(url, element.attr("id")) {
                    block(&mut parent.body, &footnote_definition(&label, body));
                } else {
                    element.list_item(parent);
                }
            }
            "dt" | "summary" => {
                block(&mut parent.body, &emphasis(body.trim(), "**"));
            }
            "input" if element.attr("type") == "checkbox" => {
                let checked = element.attributes.contains_key("checked");
                parent.body.push_str(match (task_checkbox, checked) {
                    (true, true) => "[x] ",
                    (true, false) => "[ ] ",
                    (false, true) => "☑ ",
                    (false, false) => "☐ ",
                });
            }
            "td" | "th" if element.html_body.is_some() && parent.tag == "tr" => {
                parent.table_cells.push(TableCell {
                    body: body.clone(),
                    header: element.tag == "th",
                    alignment: element.alignment(),
                });
            }
            "tr" if element.html_body.is_some() => {
                parent.table_rows.push(std::mem::take(&mut element.table_cells));
            }
            "thead" | "tbody" | "tfoot" if element.html_body.is_some() => {
                parent.table_rows.append(&mut element.table_rows);
            }
            "caption" if parent.tag == "table" => block(&mut parent.body, body),
            "table" if element.html_body.is_some() => {
                if let Some(table) = element.table_markdown() {
                    block(&mut parent.body, body);
                    block(&mut parent.body, &table);
                } else {
                    block(&mut parent.body, html.expect("table HTML"));
                }
            }
            "p" if element.has_class("admonition-title") => {
                block(&mut parent.body, &emphasis(body.trim(), "**"));
            }
            tag if is_block(tag) => block(&mut parent.body, body),
            _ => parent.body.push_str(body),
        }
    }

    fn text(&mut self, text: &str) {
        let element = self.stack.last_mut().expect("document root");
        if element.mode == Mode::Skip {
            return;
        }
        if let Some(html) = &mut element.html_body {
            html_text(html, text, element.preserve_html_lines);
        }
        match element.mode {
            Mode::Skip => (),
            Mode::Literal => element.body.push_str(text),
            Mode::Html => {
                html_text(&mut element.body, text, element.preserve_html_lines);
            }
            Mode::Markdown => {
                let mut characters = text.chars().peekable();
                while let Some(character) = characters.next() {
                    if character.is_ascii_whitespace() {
                        space(&mut element.body);
                    } else {
                        escape_character(
                            &mut element.body,
                            character,
                            characters.peek().copied(),
                        );
                    }
                }
            }
        }
    }
}

impl Visitor for Parser {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, _: Span<usize>,
        _: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.start = Element {
                    tag: String::from_utf8_lossy(name).into_owned(),
                    ..Element::default()
                };
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute = String::from_utf8_lossy(name).into_owned();
                self.start
                    .attributes
                    .entry(self.attribute.clone())
                    .or_default();
            }
            CallbackEvent::AttributeValue { value } => {
                self.start
                    .attributes
                    .entry(self.attribute.clone())
                    .or_default()
                    .push_str(&String::from_utf8_lossy(value));
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                let element = std::mem::take(&mut self.start);
                self.open(element, *self_closing);
            }
            CallbackEvent::EndTag { name } => {
                if let Some(index) = self
                    .stack
                    .iter()
                    .rposition(|item| item.tag.as_bytes() == *name)
                {
                    while self.stack.len() > index && self.stack.len() > 1 {
                        self.close();
                    }
                }
            }
            CallbackEvent::String { value } => {
                self.text(&String::from_utf8_lossy(value));
            }
            _ => (),
        }
    }
}

impl Element {
    fn alert_kind(&self) -> Option<&'static str> {
        if !(self.tag == "div" && self.has_class("admonition") || self.tag == "details") {
            return None;
        }
        self.attr("class").split_ascii_whitespace().find_map(|class| {
            match class {
                "note" | "info" | "todo" | "abstract" | "summary" | "tldr"
                    | "question" | "help" | "faq" | "example" | "quote" | "cite" => Some("NOTE"),
                "tip" | "hint" | "success" | "check" | "done" => Some("TIP"),
                "important" => Some("IMPORTANT"),
                "warning" | "attention" => Some("WARNING"),
                "caution" | "danger" | "error" | "failure" | "fail" | "missing" | "bug" => Some("CAUTION"),
                _ => None,
            }
        }).or_else(|| self.has_class("admonition").then_some("NOTE"))
    }

    fn alert(&self, kind: &str, nested: bool) -> String {
        let mut body = String::new();
        if !nested {
            // Separate the marker from blocks such as tables and thematic breaks.
            block(&mut body, &format!("[!{kind}]"));
        }
        let title = self.admonition_title.trim();
        if nested || !title.eq_ignore_ascii_case(kind) {
            block(&mut body, &emphasis(title, "**"));
        }
        block(&mut body, &self.body);
        blockquote(&body)
    }

    fn alignment(&self) -> Option<&'static str> {
        let alignment = self.attr("style").split(';').find_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            name.trim().eq_ignore_ascii_case("text-align").then_some(value.trim())
        }).unwrap_or_else(|| self.attr("align"));
        match alignment.trim_end_matches("!important").trim().to_ascii_lowercase().as_str() {
            "left" => Some(":---"),
            "center" => Some(":---:"),
            "right" => Some("---:"),
            _ => None,
        }
    }

    fn table_markdown(&self) -> Option<String> {
        if self.table_fallback {
            return None;
        }
        let width = self.table_rows.iter().map(Vec::len).max().filter(|width| *width > 0)?;
        let header = self.table_rows.first()
            .filter(|row| !row.is_empty() && row.iter().all(|cell| cell.header))
            .map_or(&[][..], Vec::as_slice);
        let mut output = String::new();
        table_row(&mut output, header, width);
        output.push('|');
        for column in 0..width {
            let alignment = self.table_rows.iter()
                .find_map(|row| row.get(column).and_then(|cell| cell.alignment))
                .unwrap_or("---");
            let _ = write!(output, " {alignment} |");
        }
        output.push('\n');
        for row in self.table_rows.iter().skip(usize::from(!header.is_empty())) {
            table_row(&mut output, row, width);
        }
        Some(output)
    }

    fn link(&self, output: &mut String, url: &str) {
        let target = link_url(url, self.attr("href"));
        let title = self.attr("title");
        let title = if title.is_empty() {
            String::new()
        } else {
            format!(
                " \"{}\"",
                title
                    .replace('&', "&amp;")
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace(['\n', '\r'], " ")
            )
        };
        let label = self.body.trim();
        if !label.is_empty() {
            if self.body.starts_with(char::is_whitespace) {
                space(output);
            }
            let _ =
                write!(output, "[{label}]({}{title})", destination(&target));
            if self.body.ends_with(char::is_whitespace) {
                space(output);
            }
        }
    }

    fn list_item(&self, parent: &mut Self) {
        let marker = if parent.tag == "ol" {
            let marker = format!("{}. ", parent.number);
            parent.number = (parent.number + 1).min(999_999_999);
            marker
        } else {
            "- ".into()
        };
        let indent = " ".repeat(marker.len());
        if !parent.body.is_empty() && !parent.body.ends_with('\n') {
            parent.body.push('\n');
        }
        if self.body.trim().is_empty() {
            parent.body.push_str(marker.trim_end());
            parent.body.push('\n');
        }
        for (index, line) in self.body.trim().lines().enumerate() {
            if index == 0 {
                parent.body.push_str(&marker);
            } else if !line.is_empty() {
                parent.body.push_str(&indent);
            }
            parent.body.push_str(line);
            parent.body.push('\n');
        }
    }

    fn attr(&self, name: &str) -> &str {
        self.attributes.get(name).map_or("", String::as_str)
    }

    fn has_class(&self, name: &str) -> bool {
        self.attr("class")
            .split_ascii_whitespace()
            .any(|class| class == name)
    }

    fn is_footnotes(&self) -> bool {
        self.has_class("footnote")
            || self.has_class("footnotes")
            || self.attributes.contains_key("data-footnotes")
    }

    fn is_decoration(&self) -> bool {
        matches!(self.tag.as_str(), "img" | "svg")
            || self.tag == "a" && self.has_class("headerlink")
            || [
                "twemoji",
                "tabbed-labels",
                "doc-labels",
                "linenodiv",
                "linenos",
                "lineno",
            ]
            .iter()
            .any(|class| self.has_class(class))
    }

    fn html(&self, url: &str, body: &str) -> String {
        let mut html = format!("<{}", self.tag);
        for (name, value) in &self.attributes {
            let value = match name.as_str() {
                "href" => link_url(url, value),
                "src" => absolute(url, value),
                _ => value.clone(),
            };
            let _ = write!(
                html,
                " {name}=\"{}\"",
                html_escape(&value)
                    .replace('\n', "&#10;")
                    .replace('\r', "&#13;")
            );
        }
        html.push('>');
        html.push_str(body);
        if !is_void(&self.tag) {
            let _ = write!(html, "</{}>", self.tag);
        }
        html
    }
}

fn table_row(output: &mut String, cells: &[TableCell], width: usize) {
    output.push('|');
    for column in 0..width {
        let body = cells.get(column).map_or("", |cell| cell.body.trim());
        let body = body.lines().map(str::trim).collect::<Vec<_>>().join("<br>");
        // GFM requires escaped pipes even inside code spans and link targets.
        let _ = write!(output, " {} |", body.replace('|', "\\|"));
    }
    output.push('\n');
}

fn footnote_label(url: &str, id: &str) -> Option<String> {
    let id = percent_decode_str(id.strip_prefix('#').unwrap_or(id)).decode_utf8().ok()?;
    let id = id.strip_prefix("fn:").or_else(|| id.strip_prefix("user-content-fn-"))?;
    let url = UriRef::parse(url).ok()?;
    // Include the page path so notes remain distinct in llms-full.txt.
    let scoped = format!("{}#{id}", url.path().as_str());
    let mut label = String::new();
    for character in scoped.chars() {
        if character.is_whitespace() || character.is_control() || matches!(character, '[' | ']' | '\\') {
            for byte in character.to_string().bytes() {
                let _ = write!(label, "%{byte:02X}");
            }
        } else {
            label.push(character);
        }
    }
    Some(label)
}

fn footnote_definition(label: &str, body: &str) -> String {
    let mut lines = body.trim().lines();
    let mut output = format!("[^{label}]: {}", lines.next().unwrap_or(""));
    for line in lines {
        output.push('\n');
        if !line.is_empty() {
            output.push_str("    ");
            output.push_str(line);
        }
    }
    output
}

/// Terminates a nonempty Markdown document with one newline.
pub(super) fn document(body: &str) -> String {
    if body.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n", body.trim())
    }
}

fn trim_spaces(body: &mut String) {
    body.truncate(body.trim_end_matches([' ', '\t']).len());
}

fn space(body: &mut String) {
    if !body.ends_with(char::is_whitespace) {
        body.push(' ');
    }
}

fn block(output: &mut String, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    output.truncate(output.trim_end().len());
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str(body.trim());
    output.push_str("\n\n");
}

fn blockquote(body: &str) -> String {
    body.trim().lines().map(|line| {
        if line.is_empty() {
            ">".into()
        } else {
            format!("> {line}")
        }
    }).collect::<Vec<_>>().join("\n")
}

fn emphasis(body: &str, delimiter: &str) -> String {
    if body.trim().is_empty() {
        return body.into();
    }
    format!(
        "{}{delimiter}{}{delimiter}{}",
        if body.starts_with(char::is_whitespace) {
            " "
        } else {
            ""
        },
        body.trim(),
        if body.ends_with(char::is_whitespace) {
            " "
        } else {
            ""
        }
    )
}

fn longest_run(text: &str, character: char) -> usize {
    text.split(|value| value != character)
        .map(str::len)
        .max()
        .unwrap_or(0)
}

fn code_span(text: &str) -> String {
    let text = text.replace(['\n', '\r'], " ");
    if text.is_empty() {
        return String::new();
    }
    let delimiter = "`".repeat(longest_run(&text, '`') + 1);
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || text.starts_with(' ')
            && text.ends_with(' ')
            && !text.trim().is_empty();
    let padding = if pad { " " } else { "" };
    format!("{delimiter}{padding}{text}{padding}{delimiter}")
}

/// Escapes text that must remain literal when read as GFM.
pub(super) fn escape(text: &str) -> String {
    let mut output = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        escape_character(&mut output, character, characters.peek().copied());
    }
    output
}

fn escape_character(
    output: &mut String, character: char, next: Option<char>,
) {
    let starts_block = if matches!(character, '-' | '+' | '=' | '.' | ')') {
        let line = output.rsplit('\n').next().unwrap_or("").trim_start();
        line.is_empty() && matches!(character, '-' | '+' | '=')
            || matches!(character, '.' | ')')
                && (1..=9).contains(&line.len())
                && line.bytes().all(|byte| byte.is_ascii_digit())
                && next.is_none_or(char::is_whitespace)
    } else {
        false
    };
    if character == '&' {
        output.push_str("&amp;");
        return;
    }
    if starts_block
        || matches!(
            character,
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '#' | '~'
        )
    {
        output.push('\\');
    }
    output.push(character);
}

/// Formats a link destination with readable spaces and Unicode.
pub(super) fn destination(url: &str) -> String {
    static TEXT_ESCAPES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"%20|(?:%[89A-Fa-f][0-9A-Fa-f])+")
            .expect("valid percent-encoding pattern")
    });

    // Decode spaces and UTF-8 text without changing encoded URL delimiters.
    let url = encode_url(url);
    let url = TEXT_ESCAPES.replace_all(&url, |captures: &Captures<'_>| {
        match percent_decode_str(&captures[0]).decode_utf8() {
            Ok(text) if !text.chars().any(char::is_control) => {
                text.into_owned()
            }
            _ => captures[0].to_owned(),
        }
    });
    format!("<{}>", url.replace('&', "&amp;"))
}

fn encode_url(url: &str) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
    const SET: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'<')
        .add(b'>')
        .add(b'"')
        .add(b'\\')
        .add(b'`');
    utf8_percent_encode(url, SET).to_string()
}

/// Resolves a URL with the same relative path rules as a browser.
pub(super) fn absolute(base: &str, value: &str) -> String {
    let encoded = encode_url(value);
    Uri::parse(base)
        .ok()
        .and_then(|base| {
            UriRef::parse(encoded.as_str())
                .ok()?
                .resolve_against(&base)
                .ok()
                .map(|uri| uri.to_string())
        })
        .unwrap_or(encoded)
}

fn link_url(base: &str, value: &str) -> String {
    if value.is_empty()
        || value.starts_with(['/', '#'])
        || UriRef::parse(value).is_ok_and(|uri| uri.scheme().is_some())
    {
        return value.into();
    }
    let mut url = absolute(base, value);
    if url.ends_with('/') {
        url.push_str("index.md");
    }
    url
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html_text(output: &mut String, text: &str, preserve_lines: bool) {
    // Blank lines end an HTML block. Preserve code line breaks as entities.
    let text = html_escape(text);
    if preserve_lines {
        output.push_str(&text.replace('\n', "&#10;").replace('\r', "&#13;"));
    } else {
        output.push_str(&text.replace(['\n', '\r'], " "));
    }
}

fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "aside"
            | "address"
            | "ul"
            | "ol"
            | "dl"
            | "dd"
            | "details"
            | "figure"
            | "figcaption"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "pre"
            | "blockquote"
    )
}

#[cfg(test)]
mod tests {
    use super::Parser;
    use crate::compat::mkdocs::html;

    fn convert(html: &str, autoclean: bool) -> String {
        let mut parser =
            Parser::new(autoclean, "https://example.org/docs/guide/".into());
        let edits = html::scan(html, &mut [&mut parser]);

        // Extraction must leave the HTML used by the site untouched.
        assert_eq!(edits, None);
        parser.finish()
    }

    #[test]
    fn converts_headings_inline_markup_and_entities() {
        let html = "<h1>Title &amp; details<a class='extra headerlink' href='#title'>¶</a></h1><p>A <strong>bold</strong> and <em>emphasized</em> word.<br>Next &lt;tag&gt; &amp;copy; [literal] *stars*.</p>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "# Title &amp; details\n\nA **bold** and *emphasized* word.  \nNext \\<tag\\> &amp;copy; \\[literal\\] \\*stars\\*.\n");
    }

    #[test]
    fn preserves_code_whitespace_and_uses_unambiguous_delimiters() {
        let html = "<p><code>`value`</code> <code> spaced </code></p><div class='language-python highlight'><pre><span></span><code><span class='n'>x</span> = &quot;```&quot;\n\n  y &lt; z\n</code></pre></div><pre><code></code></pre>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "`` `value` `` `  spaced  `\n\n````python\nx = \"```\"\n\n  y < z\n````\n\n```\n```\n");
    }

    #[test]
    fn removes_empty_code_elements_from_markdown_and_retained_html() {
        let html = concat!(
            "<h2><code class='doc-symbol doc-symbol-heading doc-symbol-function'></code> run</h2>",
            "<p>Before<code></code><code><span></span></code>after <code>run()</code>.</p>",
            "<table><tr><th>Name</th></tr><tr><td><code></code>run</td></tr></table>",
            "<table><tr><td colspan='2'><code></code>Wide</td></tr></table>",
            "<p><kbd><code></code>Esc</kbd></p>",
        );

        for autoclean in [true, false] {
            let markdown = convert(html, autoclean);

            assert_eq!(markdown, concat!(
                "## run\n\nBeforeafter `run()`.\n\n",
                "| Name |\n| --- |\n| run |\n\n",
                "<table><tr><td colspan=\"2\">Wide</td></tr></table>\n\n",
                "<kbd>Esc</kbd>\n",
            ));
        }
    }

    #[test]
    fn preserves_literal_code_markup_and_whitespace_examples() {
        let html = concat!(
            "<p>Markup: <code>&lt;code&gt;&lt;/code&gt;</code>, space: <code> </code>.</p>",
            "<pre><code class='language-html'>&lt;code&gt;&lt;/code&gt;\n</code></pre>",
            "<pre><code class='language-python'></code></pre>",
        );

        let markdown = convert(html, true);

        // Literal markup is example content, and empty fences keep their language.
        assert_eq!(markdown, concat!(
            "Markup: `<code></code>`, space: ` `.\n\n",
            "```html\n<code></code>\n```\n\n```python\n```\n",
        ));
    }

    #[test]
    fn removes_mkdocstrings_source_listings_and_keeps_signatures_and_examples() {
        let html = concat!(
            "<div class='doc-contents'><p>Description.</p>",
            "<div class='language-python doc-signature highlight'><pre><code>run()</code></pre></div>",
            "<details class='extra mkdocstrings-source' open>",
            "<summary>Source code in <code>example.py</code></summary>",
            "<div class='language-python highlight'><table class='highlighttable'><tr>",
            "<td class='linenos'><pre>1\n2</pre></td>",
            "<td class='code'><pre><code><span>def run():</span>\n    return 42</code></pre></td>",
            "</tr></table></div></details>",
            "<p>Example:</p><pre><code class='language-python'>run()</code></pre></div>",
        );

        for autoclean in [true, false] {
            let markdown = convert(html, autoclean);

            assert_eq!(markdown, "Description.\n\n```python\nrun()\n```\n\nExample:\n\n```python\nrun()\n```\n");
        }
    }

    #[test]
    fn indents_nested_lists_by_the_commonmark_marker_width() {
        let html = "<ol start='9'><li>First<ul><li>Nested</li></ul></li><li><p>Second</p><p>Paragraph</p><pre><code>a\n b\n</code></pre></li></ol>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "9. First\n\n   - Nested\n10. Second\n\n    Paragraph\n\n    ```\n    a\n     b\n    ```\n");
    }

    #[test]
    fn converts_details_definitions_and_quotes() {
        let html = concat!(
            "<details><summary>More</summary><p>Details.</p></details>",
            "<details class='example-source'><summary>Source code in <code>example.py</code></summary><pre><code>pass</code></pre></details>",
            "<dl><dt>Term</dt><dd>Meaning.</dd></dl>",
            "<blockquote><p>First</p><blockquote><p>Nested</p></blockquote><p>Last</p></blockquote>",
        );

        let markdown = convert(html, true);

        assert_eq!(markdown, "**More**\n\nDetails.\n\n**Source code in `example.py`**\n\n```\npass\n```\n\n**Term**\n\nMeaning.\n\n> First\n>\n> > Nested\n>\n> Last\n");
    }

    #[test]
    fn converts_admonitions_to_github_alerts_without_repeating_default_titles() {
        for (class, title, kind) in [
            ("note", "Note", "NOTE"),
            ("tip", "Tip", "TIP"),
            ("important", "Important", "IMPORTANT"),
            ("warning", "Warning", "WARNING"),
            ("caution", "Caution", "CAUTION"),
        ] {
            let html = format!("<div class='admonition {class}'><p class='admonition-title'>{title}</p><p>Body.</p></div>");

            for autoclean in [true, false] {
                let markdown = convert(&html, autoclean);

                assert_eq!(markdown, format!("> [!{kind}]\n>\n> Body.\n"));
            }
        }
    }

    #[test]
    fn maps_extra_admonition_types_and_preserves_custom_titles() {
        for (classes, kind) in [
            ("info", "NOTE"),
            ("todo", "NOTE"),
            ("abstract", "NOTE"),
            ("summary", "NOTE"),
            ("tldr", "NOTE"),
            ("question", "NOTE"),
            ("help", "NOTE"),
            ("faq", "NOTE"),
            ("example", "NOTE"),
            ("quote", "NOTE"),
            ("cite", "NOTE"),
            ("hint", "TIP"),
            ("success", "TIP"),
            ("check", "TIP"),
            ("done", "TIP"),
            ("attention", "WARNING"),
            ("danger", "CAUTION"),
            ("error", "CAUTION"),
            ("failure", "CAUTION"),
            ("fail", "CAUTION"),
            ("missing", "CAUTION"),
            ("bug", "CAUTION"),
            ("custom", "NOTE"),
            ("extra warning inline", "WARNING"),
            ("warningish", "NOTE"),
        ] {
            let html = format!("<div class='admonition {classes}'><p class='admonition-title'>A <code>custom</code> title</p><p>Body.</p></div>");

            let markdown = convert(&html, true);

            assert_eq!(markdown, format!("> [!{kind}]\n>\n> **A `custom` title**\n>\n> Body.\n"), "{classes}");
        }
    }

    #[test]
    fn converts_titleless_and_empty_admonitions() {
        for (html, expected) in [
            ("<div class='admonition note'><p>Body.</p></div>", "> [!NOTE]\n>\n> Body.\n"),
            ("<div class='admonition'><p>Body.</p></div>", "> [!NOTE]\n>\n> Body.\n"),
            ("<div class='admonition note'><p class='admonition-title'>Note</p></div>", "> [!NOTE]\n"),
            // The thematic break must not turn the alert marker into a heading.
            ("<div class='admonition note'><hr><p>Body.</p></div>", "> [!NOTE]\n>\n> ---\n>\n> Body.\n"),
        ] {
            let markdown = convert(html, true);

            assert_eq!(markdown, expected);
        }
    }

    #[test]
    fn converts_collapsible_admonitions_with_rich_content() {
        let html = concat!(
            "<details class='tip' open><summary>Follow these <code>steps</code></summary>",
            "<p>Read <a href='../guide/'>the guide</a>.</p>",
            "<ul><li>First</li><li>Second</li></ul>",
            "<pre><code class='language-python'>if ready:\n    run()\n</code></pre>",
            "<table><tr><th>Name</th></tr><tr><td>Value</td></tr></table>",
            "</details><p>After.</p>",
        );

        let markdown = convert(html, true);

        assert_eq!(markdown, concat!(
            "> [!TIP]\n>\n> **Follow these `steps`**\n>\n",
            "> Read [the guide](<https://example.org/docs/guide/index.md>).\n>\n",
            "> - First\n> - Second\n>\n",
            "> ```python\n> if ready:\n>     run()\n> ```\n>\n",
            "> | Name |\n> | --- |\n> | Value |\n\nAfter.\n",
        ));
    }

    #[test]
    fn preserves_nested_admonitions_as_quotes_with_their_titles() {
        let admonition = "<div class='admonition tip'><p class='admonition-title'>Tip</p><p>Body.</p></div>";

        // GitHub cannot render nested alerts, but the title and nesting must survive.
        for (html, expected) in [
            (format!("<div class='admonition note'>{admonition}</div>"), "> [!NOTE]\n>\n> > **Tip**\n> >\n> > Body.\n"),
            (format!("<blockquote>{admonition}</blockquote>"), "> > **Tip**\n> >\n> > Body.\n"),
            (format!("<ul><li>{admonition}</li></ul>"), "- > **Tip**\n  >\n  > Body.\n"),
        ] {
            let markdown = convert(&html, true);

            assert_eq!(markdown, expected);
        }
    }

    #[test]
    fn converts_tables_to_gfm() {
        let html = "<table><thead><tr><th>Name</th></tr></thead><tbody><tr><td><strong>A &amp; B</strong><br><code>x_y</code></td></tr></tbody></table><p>After</p>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "| Name |\n| --- |\n| **A &amp; B**<br>`x_y` |\n\nAfter\n");
    }

    #[test]
    fn preserves_table_alignment_pipes_and_paragraphs() {
        let html = concat!(
            "<table><caption>Results</caption><thead><tr>",
            "<th style='text-align: left'>Name</th><th align='center'>Type</th>",
            "<th style='text-align: right'>Notes</th></tr></thead><tbody><tr>",
            "<td>A|B</td><td><code>str | None</code></td>",
            "<td><p>First</p><p><a href='../topic/'>Second</a></p></td>",
            "</tr></tbody></table>",
        );

        let markdown = convert(html, true);

        assert_eq!(markdown, concat!(
            "Results\n\n| Name | Type | Notes |\n| :--- | :---: | ---: |\n",
            "| A\\|B | `str \\| None` | First<br><br>[Second](<https://example.org/docs/topic/index.md>) |\n",
        ));
    }

    #[test]
    fn gives_headerless_tables_an_empty_header_and_pads_short_rows() {
        let html = "<table><tr><td>A</td><td>B</td></tr><tr><td>C</td></tr></table>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "|  |  |\n| --- | --- |\n| A | B |\n| C |  |\n");
    }

    #[test]
    fn retains_complex_tables_as_html() {
        let cases = [
            (
                "<table><tr><td colspan='2'>Wide</td></tr></table>",
                "<table><tr><td colspan=\"2\">Wide</td></tr></table>\n",
            ),
            (
                "<table><tr><td><pre><code>a\n\nb</code></pre></td></tr></table>",
                "<table><tr><td><pre><code>a&#10;&#10;b</code></pre></td></tr></table>\n",
            ),
            (
                "<table><tr><td><table><tr><td>Nested</td></tr></table></td></tr></table>",
                "<table><tr><td><table><tr><td>Nested</td></tr></table></td></tr></table>\n",
            ),
            (
                "<table><tr><td><div class='admonition note'><p>Body.</p></div></td></tr></table>",
                "<table><tr><td><div class=\"admonition note\"><p>Body.</p></div></td></tr></table>\n",
            ),
        ];

        for (html, expected) in cases {
            let markdown = convert(html, true);

            assert_eq!(markdown, expected);
        }
    }

    #[test]
    fn converts_strikethrough_and_preserves_literal_tildes() {
        let html = "<p><del>Old <strong>value</strong></del>, <s>removed</s>, and ~literal~.</p>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "~~Old **value**~~, ~~removed~~, and \\~literal\\~.\n");
    }

    #[test]
    fn converts_plain_and_custom_task_lists() {
        let html = concat!(
            "<ul><li><input type='checkbox' checked>Done</li>",
            "<li><label class='task-list-control'><input type='checkbox'>",
            "<span class='task-list-indicator'></span></label>Todo</li></ul>",
            "<p>Separate <input type='checkbox' checked>checkbox.</p>",
        );

        for autoclean in [true, false] {
            let markdown = convert(html, autoclean);

            assert_eq!(markdown, "- [x] Done\n- [ ] Todo\n\nSeparate ☑ checkbox.\n");
        }
    }

    #[test]
    fn converts_footnotes_with_repeated_references_and_multiple_blocks() {
        let html = concat!(
            "<p>First<sup id='fnref:note'><a class='footnote-ref' href='#fn:note'>1</a></sup>",
            " and again<sup id='fnref2:note'><a class='footnote-ref' href='#fn:note'>1</a></sup>.</p>",
            "<div class='footnote'><hr><ol><li id='fn:note'><p>A <em>note</em>.</p>",
            "<p>More detail.</p><ul><li>One</li><li>Two</li></ul>",
            "<p><a class='footnote-backref' href='#fnref:note'>↩</a>",
            "<a class='footnote-backref' href='#fnref2:note'>↩</a></p>",
            "</li></ol></div>",
        );

        for autoclean in [true, false] {
            let markdown = convert(html, autoclean);

            assert_eq!(markdown, concat!(
                "First[^/docs/guide/#note] and again[^/docs/guide/#note].\n\n",
                "[^/docs/guide/#note]: A *note*.\n\n    More detail.\n\n    - One\n    - Two\n",
            ));
        }
    }

    #[test]
    fn converts_github_footnote_markup() {
        let html = concat!(
            "<p>Note<sup><a data-footnote-ref href='#user-content-fn-1'>1</a></sup>.</p>",
            "<section data-footnotes><h2 id='footnote-label'>Footnotes</h2><ol>",
            "<li id='user-content-fn-1'><p>Body. <a data-footnote-backref href='#user-content-fnref-1'>↩</a></p></li>",
            "</ol></section>",
        );

        let markdown = convert(html, true);

        assert_eq!(markdown, "Note[^/docs/guide/#1].\n\n[^/docs/guide/#1]: Body.\n");
    }

    #[test]
    fn cleans_controls_and_highlighted_code_without_losing_text() {
        let html = "<div class='tabbed-labels'><label>Tab</label></div><p>Text<span class='twemoji'><svg>icon</svg></span><span class='doc-labels'>property</span><a href='x'><img src='x'>badge</a></p><table class='highlighttable'><tr><td class='linenos'><div class='linenodiv'><pre>1\n2</pre></div></td><td class='code'><div class='highlight'><pre><code>x\ny\n</code></pre></div></td></tr></table><script>bad()</script><style>body{}</style>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "Text\n\n```\nx\ny\n```\n");
    }

    #[test]
    fn keeps_images_and_permalinks_when_autoclean_is_disabled() {
        let html = "<h2>Heading<a href='#heading' class='headerlink'>¶</a></h2><p><img alt='An [image]' src='../images/a b.png'></p>";

        let markdown = convert(html, false);

        assert_eq!(markdown, "## Heading[¶](<#heading>)\n\n![An \\[image\\]](<https://example.org/docs/images/a b.png>)\n");
    }

    #[test]
    fn resolves_relative_links_and_preserves_absolute_targets() {
        let html = "<p><a href='../topic/'>Topic</a> <a href='../other.html?x=1&amp;y=2#part'>Other</a> <a href='#local'>Local</a> <a href='/root'>Root</a> <a href='https://elsewhere.org/'>External</a></p>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "[Topic](<https://example.org/docs/topic/index.md>) [Other](<https://example.org/docs/other.html?x=1&amp;y=2#part>) [Local](<#local>) [Root](</root>) [External](<https://elsewhere.org/>)\n");
    }

    #[test]
    fn keeps_spaces_and_unicode_readable_in_link_destinations() {
        let html = concat!(
            "<p><a href='../caf%C3%A9%20notes/'>Encoded</a></p>",
            "<p><a href='../café notes/'>Literal</a></p>",
            "<p><a href='https://example.org/caf%c3%a9%20notes)'>External</a></p>",
            "<p><a href='/café notes/'>Root</a></p>",
            "<p><a href='#caf%C3%A9%20notes'>Fragment</a></p>",
            "<p><a href='?q=caf%C3%A9%20notes&amp;next=1'>Query</a></p>",
            "<p><img alt='Café' src='../images/caf%C3%A9%20notes.png'></p>",
            "<p><a href='/notes%20%F0%9F%8C%8D'>Unicode</a></p>",
        );

        let markdown = convert(html, false);

        assert_eq!(
            markdown,
            concat!(
                "[Encoded](<https://example.org/docs/café notes/index.md>)\n\n",
                "[Literal](<https://example.org/docs/café notes/index.md>)\n\n",
                "[External](<https://example.org/café notes)>)\n\n",
                "[Root](</café notes/>)\n\n",
                "[Fragment](<#café notes>)\n\n",
                "[Query](<https://example.org/docs/guide/?q=café notes&amp;next=1>)\n\n",
                "![Café](<https://example.org/docs/images/café notes.png>)\n\n",
                "[Unicode](</notes 🌍>)\n",
            )
        );
    }

    #[test]
    fn preserves_url_delimiters_and_unreadable_percent_encoded_bytes() {
        let targets = [
            // Decoding reserved characters would change the link target.
            "/a%2Fb%23c%3Fd%25e",
            "https://example.org/?value=a%26b%3Dc%2Bd",
            "/%2520%25C3%25A9",
            // Invalid UTF-8 and control characters must stay encoded.
            "/%ff%80%F0%28%8C%28",
            "/%00%09%0A%0D%7F%C2%85",
            // Angle brackets and backslashes affect Markdown parsing.
            "/%3Ctag%3E%5C",
        ];

        for target in targets {
            let html = format!("<p><a href='{target}'>Link</a></p>");

            let markdown = convert(&html, true);

            assert_eq!(markdown, format!("[Link](<{target}>)\n"));
        }
    }

    #[test]
    fn preserves_literal_block_markers_and_recovers_unclosed_elements() {
        let html = "<p>1. Literal</p><p># Literal</p><p>--- Literal</p><p>+ Literal</p><p>= Literal</p><div><p>Tail <strong>bold";

        let markdown = convert(html, true);

        assert_eq!(markdown, "1\\. Literal\n\n\\# Literal\n\n\\--- Literal\n\n\\+ Literal\n\n\\= Literal\n\nTail **bold**\n");
    }

    #[test]
    fn keeps_version_numbers_readable() {
        let html = concat!(
            "<p>Demo version: <strong>0.1.0</strong>.</p>",
            "<p>0.1.0</p>",
            "<p><em>0&#46;1.0</em> and <a href='#release'>0.1.0</a>.</p>",
        );

        let markdown = convert(html, true);

        assert_eq!(
            markdown,
            concat!(
                "Demo version: **0.1.0**.\n\n",
                "0.1.0\n\n",
                "*0.1.0* and [0.1.0](<#release>).\n",
            )
        );
    }

    #[test]
    fn distinguishes_ordered_list_markers_from_numeric_text() {
        let cases = [
            ("<p>1. Literal</p>", "1\\. Literal\n"),
            ("<p>2) Literal</p>", "2\\) Literal\n"),
            ("<p>1.\tLiteral</p>", "1\\. Literal\n"),
            ("<p>1.\nLiteral</p>", "1\\. Literal\n"),
            ("<p>1.</p>", "1\\.\n"),
            ("<p>2)</p>", "2\\)\n"),
            // Trailing Unicode whitespace is trimmed from the paragraph.
            ("<p>1.&nbsp;</p>", "1\\.\n"),
            // An inline element can supply the space after a list marker.
            ("<p>1.<em> Literal</em></p>", "1\\. *Literal*\n"),
            ("<p>2)text</p>", "2)text\n"),
            ("<p>1.2</p>", "1.2\n"),
            // CommonMark list markers contain at most nine digits.
            ("<p>123456789. Literal</p>", "123456789\\. Literal\n"),
            ("<p>1234567890. Literal</p>", "1234567890. Literal\n"),
        ];

        for (html, expected) in cases {
            let markdown = convert(html, true);

            assert_eq!(markdown, expected, "{html}");
        }
    }

    #[test]
    fn escapes_heading_suffixes_and_entities_in_link_attributes() {
        let html = "<h1>Heading #</h1><p><a href='?a=&amp;copy;' title='A &amp;copy;'>Link</a></p><ul><li></li><li>Next</li></ul>";

        let markdown = convert(html, true);

        assert_eq!(markdown, "# Heading \\#\n\n[Link](<https://example.org/docs/guide/?a=&amp;copy;> \"A &amp;copy;\")\n\n-\n- Next\n");
    }
}
