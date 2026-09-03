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

//! Native slug functions shared by generated site structures.

use icu_casemap::CaseMapper;
use icu_locale_core::LanguageIdentifier;
use icu_normalizer::{ComposingNormalizer, DecomposingNormalizer};

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Creates the default Unicode-aware Material slug.
///
/// This mirrors `pymdownx.slugs.slugify(case = "lower")`: input is normalized
/// to NFC, HTML tags are removed, surrounding whitespace is stripped, Unicode
/// lowercase mapping is applied, and only word characters, dashes, and spaces
/// are retained. Each literal space becomes the configured separator.
pub fn unicode(value: &str, separator: &str) -> String {
    let stripped = strip_html(value);
    let normalized = ComposingNormalizer::new_nfc().normalize(&stripped);
    let normalized = normalized.trim();
    let cased = CaseMapper::new()
        .lowercase_to_string(normalized, &LanguageIdentifier::UNKNOWN)
        .into_owned();
    let mut output = String::with_capacity(cased.len());
    for character in cased.chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '-') {
            output.push(character);
        } else if character == ' ' {
            output.push_str(separator);
        }
    }
    output
}

/// Creates Python Markdown's default ASCII NFKD slug.
pub fn ascii(value: &str, separator: &str) -> String {
    let normalized = DecomposingNormalizer::new_nfkd().normalize(value);
    let filtered = normalized
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_whitespace()
                || matches!(character, '_' | '-')
        })
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let mut output = String::with_capacity(filtered.len());
    let mut inside_separator = false;
    for character in filtered.trim().chars() {
        if character.is_whitespace() || separator.contains(character) {
            if !inside_separator {
                output.push_str(separator);
                inside_separator = true;
            }
        } else {
            inside_separator = false;
            output.push(character);
        }
    }
    output
}

/// Removes HTML tags using pymdownx's permissive non-nesting semantics.
fn strip_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        output.push_str(&rest[..start]);
        let candidate = &rest[start + 1..];
        if let Some(end) = candidate.find('>') {
            rest = &candidate[end + 1..];
        } else {
            output.push_str(&rest[start..]);
            return output;
        }
    }
    output.push_str(rest);
    output
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{ascii, unicode};

    #[test]
    fn matches_material_default_unicode_slugification() {
        let cases = [
            ("Über Café 東京", "über-café-東京"),
            ("<b>A  Straße</b>", "a--straße"),
            ("ΣΣ", "σς"),
            (" spaced ", "spaced"),
            ("punctuation!? remains_ok", "punctuation-remains_ok"),
            ("tab\tremoved", "tabremoved"),
            ("a<b", "ab"),
            ("!!!", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(unicode(input, "-"), expected, "input: {input}");
        }
    }

    #[test]
    fn uses_the_configured_separator_without_collapsing_spaces() {
        assert_eq!(unicode("A  B", "_"), "a__b");
    }

    #[test]
    fn matches_python_markdown_ascii_slugification() {
        assert_eq!(ascii("Über Café 東京", "-"), "uber-cafe");
        assert_eq!(ascii(" A---B ", "-"), "a-b");
    }
}
