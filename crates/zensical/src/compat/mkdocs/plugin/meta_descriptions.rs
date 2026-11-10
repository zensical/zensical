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

//! Native mkdocs-meta-descriptions-plugin compatibility.

use anyhow::{bail, Result};
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use std::sync::Arc;

use crate::compat::mkdocs::html::{Editor, Visitor};
use crate::config::{plugins::MetaDescriptionsPluginConfig, Config};
use crate::structure::dynamic::Dynamic;
use crate::structure::markdown::Markdown;
use crate::structure::page::Page;

mod output;

pub use output::Document;

/// Resolved settings shared by extraction and final output processing.
#[derive(Clone, Debug)]
pub struct MetaDescriptions {
    settings: Arc<MetaDescriptionsPluginConfig>,
    site_url: String,
}

/// First eligible paragraph before the first subsection heading.
#[derive(Default)]
pub struct Parser {
    elements: Vec<Element>,
    start: Element,
    class_attribute: bool,
    paragraph: Option<usize>,
    text: String,
    done: bool,
}

/// Open element and the attributes needed for paragraph selection.
#[derive(Default)]
struct Element {
    name: String,
    admonition: bool,
}

impl MetaDescriptions {
    /// Settings are resolved once per workflow.
    pub fn new(config: &Config) -> Self {
        Self {
            settings: Arc::new(
                config.project.plugins.meta_descriptions.config.clone(),
            ),
            site_url: config.project.site_url.clone().unwrap_or_default(),
        }
    }

    /// A parser is created only when an explicit description is absent.
    pub fn parser(&self, markdown: &Markdown) -> Option<Parser> {
        (self.settings.enabled
            && !has_description(markdown.meta.get("description")))
        .then(Parser::default)
    }

    /// A generated description is inserted after the shared HTML pass.
    pub fn apply(&self, parser: Parser, markdown: &mut Markdown) -> bool {
        let description = parser.finish();
        if description.is_empty()
            || (self.settings.fallback_if_short
                && description.chars().count() < self.settings.min_length)
        {
            return false;
        }
        let description = if self.settings.trim {
            shorten(&description, self.settings.max_length)
        } else {
            description
        };
        markdown
            .insert_meta("description".into(), Dynamic::String(description));
        true
    }

    /// Checks are run outside the Markdown cache so cached builds are checked.
    pub fn check(&self, page: &Page, strict: bool) -> Result<()> {
        if !self.settings.enabled || !self.settings.enable_checks {
            return Ok(());
        }
        let length = page
            .meta
            .get("description")
            .filter(|value| has_description(Some(value)))
            .map_or(0, |value| value.to_string().chars().count());
        let warning = if length == 0 {
            Some(format!("Meta description not found: {}", page.source()))
        } else if length < self.settings.min_length {
            Some(length_warning(
                self.settings.min_length - length,
                "shorter",
                self.settings.min_length,
                page,
            ))
        } else if length > self.settings.max_length {
            Some(length_warning(
                length - self.settings.max_length,
                "longer",
                self.settings.max_length,
                page,
            ))
        } else {
            None
        };
        if let Some(warning) = warning {
            eprintln!("WARNING -  meta-descriptions: {warning}");
            if strict {
                bail!("meta-descriptions: {warning}");
            }
        }
        Ok(())
    }
}

impl Parser {
    /// The paragraph is escaped before length checks, matching the plugin.
    fn finish(self) -> String {
        // Upstream html.escape is applied before textwrap.shorten. This order
        // is retained because escaped entities count toward the length limits.
        self.text
            .trim()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    }

    fn open(&mut self, self_closing: bool) {
        let element = std::mem::take(&mut self.start);
        if matches!(element.name.as_str(), "h2" | "h3" | "h4" | "h5" | "h6") {
            self.done = true;
            return;
        }
        if element.name == "p" {
            if self.paragraph.is_some() {
                self.done = true;
                return;
            }
            let excluded = self.elements.last().is_some_and(|parent| {
                parent.name == "div" && parent.admonition
            });
            if !excluded {
                self.paragraph = Some(self.elements.len());
            }
        }
        if !self_closing && !is_void(&element.name) {
            self.elements.push(element);
        }
    }
}

impl Visitor for Parser {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, _: Span<usize>,
        _: &mut Editor<'_>,
    ) {
        if self.done {
            return;
        }
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.start = Element {
                    name: String::from_utf8_lossy(name).into_owned(),
                    admonition: false,
                };
                self.class_attribute = false;
            }
            CallbackEvent::AttributeName { name } => {
                self.class_attribute = *name == b"class";
            }
            CallbackEvent::AttributeValue { value } if self.class_attribute => {
                self.start.admonition = value
                    .split(u8::is_ascii_whitespace)
                    .any(|class| class == b"admonition");
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                self.open(*self_closing);
            }
            CallbackEvent::EndTag { name } => {
                if let Some(index) = self
                    .elements
                    .iter()
                    .rposition(|element| element.name.as_bytes() == *name)
                {
                    if self.paragraph.is_some_and(|depth| index <= depth) {
                        self.done = true;
                    }
                    self.elements.truncate(index);
                }
            }
            // Script, style, and template contents are excluded as by
            // BeautifulSoup.get_text. Inline text is joined without spaces.
            CallbackEvent::String { value }
                if self.paragraph.is_some()
                    && !self.elements.iter().any(|element| {
                        matches!(
                            element.name.as_str(),
                            "script" | "style" | "template"
                        )
                    }) =>
            {
                self.text.push_str(&String::from_utf8_lossy(value));
            }
            _ => {}
        }
    }
}

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

fn has_description(value: Option<&Dynamic>) -> bool {
    match value {
        None
        | Some(Dynamic::Null | Dynamic::Bool(false) | Dynamic::Integer(0)) => {
            false
        }
        Some(Dynamic::String(value)) => !value.is_empty(),
        Some(Dynamic::List(value)) => !value.is_empty(),
        Some(Dynamic::Map(value)) => !value.is_empty(),
        Some(Dynamic::Float(value)) => value.get() != 0.0,
        Some(_) => true,
    }
}

fn shorten(text: &str, width: usize) -> String {
    // The first wrapped line is used by textwrap.shorten(..., placeholder="").
    // Chunks wider than the entire line are split, including when earlier
    // chunks already occupy part of the line. Other chunks remain whole.
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut output = String::new();
    let mut length = 0;
    for chunk in chunks(&collapsed) {
        let size = chunk.chars().count();
        if length + size > width {
            if size > width {
                let mut end = width - length;
                let chars = chunk.chars().collect::<Vec<_>>();
                if let Some(hyphen) =
                    chars[..end].iter().rposition(|character| *character == '-')
                    && hyphen > 0
                    && chars[..hyphen].iter().any(|character| *character != '-')
                {
                    end = hyphen + 1;
                }
                output.extend(chars[..end].iter());
            }
            break;
        }
        output.push_str(chunk);
        length += size;
    }
    output.trim_end().into()
}

/// Whitespace, hyphenated words, and double hyphens are split as by textwrap.
fn chunks(text: &str) -> Vec<&str> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let word =
        |character: char| character.is_alphanumeric() || character == '_';
    let letter = |character: char| word(character) && !character.is_numeric();
    let at = |index: usize| chars.get(index).map(|(_, character)| *character);
    let mut chunks = Vec::new();
    let mut start = 0;
    for (index, &(byte, character)) in chars.iter().enumerate() {
        let end = byte + character.len_utf8();
        if character == ' ' {
            if start < byte {
                chunks.push(&text[start..byte]);
            }
            chunks.push(&text[byte..end]);
            start = end;
        } else if character == '-' {
            if index > 0 && at(index - 1) == Some('-') {
                continue;
            }
            // The double-hyphen alternative in TextWrapper.wordsep_re is
            // bounded by a word or punctuation on the left and a word on the right.
            let mut run_end = index + 1;
            while at(run_end) == Some('-') {
                run_end += 1;
            }
            let previous = index.checked_sub(1).and_then(at);
            if run_end > index + 1
                && previous.is_some_and(|value| {
                    word(value) || "!\"'&.,?".contains(value)
                })
                && at(run_end).is_some_and(word)
            {
                if start < byte {
                    chunks.push(&text[start..byte]);
                }
                let run_byte = chars[run_end].0;
                chunks.push(&text[byte..run_byte]);
                start = run_byte;
            } else if byte >= start {
                let before = index >= 2
                    && at(index - 1).is_some_and(letter)
                    && (at(index - 2).is_some_and(letter)
                        || (index >= 3
                            && at(index - 2) == Some('-')
                            && at(index - 3).is_some_and(letter)));
                let after = at(index + 1).is_some_and(letter)
                    && (at(index + 2).is_some_and(letter)
                        || (at(index + 2) == Some('-')
                            && at(index + 3).is_some_and(letter)));
                if before && after {
                    chunks.push(&text[start..end]);
                    start = end;
                }
            }
        }
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

fn length_warning(
    difference: usize, comparative: &str, limit: usize, page: &Page,
) -> String {
    let plural = if difference == 1 { "" } else { "s" };
    format!("Meta description {difference} character{plural} {comparative} than {limit}: {}", page.source())
}

#[cfg(test)]
mod tests {
    use super::{shorten, Parser};
    use crate::compat::mkdocs::html;

    fn paragraph(content: &str) -> String {
        let mut parser = Parser::default();
        html::scan(content, &mut [&mut parser]);
        parser.finish()
    }

    #[test]
    fn first_introductory_paragraph_is_selected() {
        // Headings and admonition paragraphs are excluded from the introduction.
        let html = "<h1>Title</h1><div class='note admonition'><p>Note</p></div>\
            <p> First <em>intro</em> &amp; &#x1f980;.</p><p>Second</p><h2>Section</h2>";

        assert_eq!(paragraph(html), "First intro &amp; 🦀.");
        assert_eq!(paragraph("<h1>Title</h1><H2>Section</H2><p>Later</p>"), "");
        assert_eq!(paragraph("<p></p><p>Second</p>"), "");
    }

    #[test]
    fn nested_admonition_content_matches_the_upstream_selector() {
        // Only direct paragraph children of div.admonition are excluded.
        let html =
            "<div class='admonition'><p>Skip</p><blockquote><p>Include</p>\
            </blockquote></div><p>Later</p>";

        assert_eq!(paragraph(html), "Include");
    }

    #[test]
    fn inline_text_and_whitespace_are_preserved_without_trimming() {
        let html = "<p>  café<br>naïve\n <code>code</code><!-- comment -->\
            <script>hidden</script><style>hidden</style><template>hidden</template>\
            &lt; &gt; &quot; &#39;  </p>";

        assert_eq!(paragraph(html), "cafénaïve\n code&lt; &gt; &quot; &#x27;");
    }

    #[test]
    fn shortening_matches_textwrap_chunks_and_unicode_lengths() {
        assert_eq!(shorten(" café\n naïve 🦀 extra ", 12), "café naïve 🦀");
        assert_eq!(shorten("longword tail", 4), "long");
        assert_eq!(shorten("a longword tail", 4), "a lo");
        assert_eq!(shorten("a longword tail", 8), "a");
        assert_eq!(shorten("hello-world tail", 8), "hello-");
        assert_eq!(shorten("foo--bar tail", 5), "foo--");
        assert_eq!(shorten("one two", 0), "");
        assert_eq!(shorten(" one\n two ", 100), "one two");
    }
}
