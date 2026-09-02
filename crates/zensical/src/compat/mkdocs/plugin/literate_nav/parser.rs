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

//! Native extraction of literate navigation lists from rendered HTML.

use anyhow::{bail, Result};
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Span, Tokenizer};
use std::collections::BTreeMap;
use std::convert::Infallible;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// One parsed literate-navigation item before path resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// Explicit Markdown link.
    Reference {
        title: Option<String>,
        target: String,
    },
    /// Named section with ordered children.
    Section { title: String, children: Vec<Item> },
    /// Bare wildcard pattern.
    Wildcard(String),
}

/// Minimal HTML node retained while extracting one Markdown list.
#[derive(Clone, Debug)]
enum Node {
    Element(Element),
    Text(String),
    Comment,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One parsed HTML element.
#[derive(Clone, Debug)]
struct Element {
    name: String,
    attributes: BTreeMap<String, String>,
    children: Vec<Node>,
}

/// Streaming tree builder for Python-Markdown's well-formed HTML.
#[derive(Default)]
struct Builder {
    root: Vec<Node>,
    stack: Vec<Element>,
    pending: Option<Element>,
    attribute: Option<String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Builder {
    /// Records one tokenizer event.
    fn event(&mut self, event: CallbackEvent<'_>) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.pending = Some(Element {
                    name: String::from_utf8_lossy(name).into_owned(),
                    attributes: BTreeMap::new(),
                    children: Vec::new(),
                });
                self.attribute = None;
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute =
                    Some(String::from_utf8_lossy(name).into_owned());
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(name) = self.attribute.take()
                    && let Some(element) = &mut self.pending
                {
                    element.attributes.insert(
                        name,
                        String::from_utf8_lossy(value).into_owned(),
                    );
                }
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some(element) = self.pending.take() {
                    if self_closing || is_void(&element.name) {
                        self.push(Node::Element(element));
                    } else {
                        self.stack.push(element);
                    }
                }
                self.attribute = None;
            }
            CallbackEvent::EndTag { name } => {
                let name = String::from_utf8_lossy(name);
                if let Some(index) =
                    self.stack.iter().rposition(|element| element.name == name)
                {
                    while self.stack.len() > index {
                        let element = self.stack.pop().expect("length checked");
                        self.push(Node::Element(element));
                    }
                }
            }
            CallbackEvent::String { value } => {
                let value = String::from_utf8_lossy(value).into_owned();
                if !value.is_empty() {
                    self.push(Node::Text(value));
                }
            }
            CallbackEvent::Comment { .. } => self.push(Node::Comment),
            CallbackEvent::Doctype { .. } | CallbackEvent::Error(_) => {}
        }
    }

    /// Appends one node to the current parent.
    fn push(&mut self, node: Node) {
        let children = self
            .stack
            .last_mut()
            .map_or(&mut self.root, |element| &mut element.children);
        if let Node::Text(value) = &node
            && let Some(Node::Text(previous)) = children.last_mut()
        {
            previous.push_str(value);
        } else {
            children.push(node);
        }
    }

    /// Completes any still-open elements.
    fn finish(mut self) -> Vec<Node> {
        while let Some(element) = self.stack.pop() {
            self.push(Node::Element(element));
        }
        self.root
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Extracts the captured root Markdown list.
pub fn parse(html: &str) -> Result<Option<Vec<Item>>> {
    let mut builder = Builder::default();
    {
        let emitter =
            CallbackEmitter::new(|event: CallbackEvent<'_>, _: Span<usize>| {
                builder.event(event);
                None::<Infallible>
            });
        Tokenizer::new_with_emitter(html, emitter)
            .finish()
            .expect("string input is infallible");
    }
    let root = builder.finish();
    let mut nodes = root.iter().filter(|node| match node {
        Node::Text(value) => !value.trim().is_empty(),
        Node::Comment => false,
        Node::Element(_) => true,
    });
    let list = match nodes.next() {
        None => return Ok(None),
        Some(Node::Element(list)) => list,
        Some(_) => bail!("captured literate navigation is not an element"),
    };
    if !is_list(&list.name) {
        bail!("captured literate navigation is not a list")
    }
    if nodes.next().is_some() {
        bail!("captured literate navigation contains multiple root elements")
    }
    Ok(Some(parse_list(list)?))
}

/// Parses one generated list element.
fn parse_list(list: &Element) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for node in &list.children {
        match node {
            Node::Element(element) if element.name == "li" => {
                items.push(parse_item(element)?);
            }
            Node::Text(value) if value.trim().is_empty() => {}
            Node::Comment => {}
            _ => bail!("literate navigation lists may only contain items"),
        }
    }
    Ok(items)
}

/// Parses one list item using mkdocs-literate-nav's structural rules.
fn parse_item(item: &Element) -> Result<Item> {
    let mut title = None;
    let mut elements = Vec::new();
    let mut saw_element = false;
    for node in &item.children {
        match node {
            Node::Text(value) if !saw_element => {
                title.get_or_insert_with(String::new).push_str(value);
            }
            // Python-Markdown's serializer inserts formatting newlines around
            // nested lists after the treeprocessor boundary used upstream.
            Node::Text(value) if !value.trim().is_empty() => {
                bail!(
                    "expected no text after an inline navigation element, but got {value:?}"
                )
            }
            Node::Element(element) => {
                saw_element = true;
                elements.push(element);
            }
            Node::Comment => {
                saw_element = true;
            }
            Node::Text(_) => {}
        }
    }

    let mut elements = elements.into_iter();
    let mut target = None;
    let first = elements.next();
    let next = if title.as_deref().is_none_or(str::is_empty)
        && first.is_some_and(|element| element.name == "a")
    {
        let anchor = first.expect("checked above");
        if let Some(href) = anchor.attributes.get("href")
            && !href.is_empty()
        {
            target = Some(href.clone());
            title = Some(text(anchor));
        }
        elements.next()
    } else {
        first
    };

    let mut children = None;
    let remaining = if next.is_some_and(|element| is_list(&element.name)) {
        children = Some(parse_list(next.expect("checked above"))?);
        elements.next()
    } else {
        next
    };
    if let Some(element) = remaining.or_else(|| elements.next()) {
        bail!("expected no more elements, but got <{}>", element.name)
    }

    let Some(title) = title.filter(|title| !title.trim().is_empty()) else {
        bail!("did not find any title specified")
    };
    let title = decode_text(&title);
    if let Some(mut children) = children {
        if let Some(target) = target {
            children.insert(0, Item::Reference { title: None, target });
        }
        return Ok(Item::Section { title, children });
    }
    if let Some(target) = target {
        return Ok(Item::Reference { title: Some(title), target });
    }
    if title.contains('*') {
        return Ok(Item::Wildcard(title));
    }
    bail!("did not find any item or section content specified")
}

/// Collects decoded descendant text.
fn text(element: &Element) -> String {
    fn collect(nodes: &[Node], output: &mut String) {
        for node in nodes {
            match node {
                Node::Element(element) => collect(&element.children, output),
                Node::Text(value) => output.push_str(value),
                Node::Comment => {}
            }
        }
    }
    let mut output = String::new();
    collect(&element.children, &mut output);
    decode_text(&output)
}

/// Decodes the lossless text transport installed by the fragment renderer.
fn decode_text(value: &str) -> String {
    const ESCAPE: char = '\u{f0000}';
    let mut input = value.chars();
    let mut output = String::with_capacity(value.len());
    while let Some(current) = input.next() {
        if current != ESCAPE {
            output.push(current);
            continue;
        }
        match input.next() {
            Some('A') => output.push('&'),
            Some('S') | None => output.push(ESCAPE),
            Some(next) => {
                output.push(ESCAPE);
                output.push(next);
            }
        }
    }
    output
}

/// Returns whether the element is a Markdown list.
fn is_list(name: &str) -> bool {
    matches!(name, "ul" | "ol")
}

/// Returns whether an HTML element is void.
fn is_void(name: &str) -> bool {
    matches!(
        name,
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

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{parse, Item};

    #[test]
    fn extracts_the_captured_root_list() {
        let html =
            "<ol><li>Guide<ul><li><a href=\"a.md\">A</a></li></ul></li></ol>";
        assert_eq!(
            parse(html).unwrap(),
            Some(vec![Item::Section {
                title: "Guide".into(),
                children: vec![Item::Reference {
                    title: Some("A".into()),
                    target: "a.md".into(),
                }],
            }])
        );
    }

    #[test]
    fn retains_section_index_and_decodes_titles() {
        let html = concat!(
            "<ul><li><a href=\"index.md\">A&amp;amp;B</a>",
            "<ul><li>*.md</li></ul></li></ul>"
        );
        assert_eq!(
            parse(html).unwrap(),
            Some(vec![Item::Section {
                title: "A&amp;B".into(),
                children: vec![
                    Item::Reference {
                        title: None,
                        target: "index.md".into(),
                    },
                    Item::Wildcard("*.md".into()),
                ],
            }])
        );
    }

    #[test]
    fn decodes_lossless_fragment_text() {
        let html = "<ul><li><a href=\"a.md\">a\u{f0000}Aamp;b</a></li></ul>";
        assert_eq!(
            parse(html).unwrap(),
            Some(vec![Item::Reference {
                title: Some("a&amp;b".into()),
                target: "a.md".into(),
            }])
        );
    }

    #[test]
    fn rejects_a_nested_list_without_a_section_title() {
        let html =
            "<ol><li>\n<ul><li><a href=\"a.md\">A</a></li></ul></li></ol>";
        assert!(parse(html)
            .unwrap_err()
            .to_string()
            .contains("did not find any title"));
    }
}
