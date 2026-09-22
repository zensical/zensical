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

//! Native Material-compatible post read-time calculation.

use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use regex::Regex;
use std::sync::LazyLock;

use crate::compat::mkdocs::html::{self, Editor, Visitor};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Text and image facts accumulated during one HTML scan.
#[derive(Default)]
struct Readtime {
    /// Visible text included in the word count.
    text: String,
    /// Images contributing decreasing reading-time penalties.
    images: usize,
    /// Depth inside elements excluded from the word count.
    skipped: usize,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Visitor for Readtime {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, _span: Span<usize>,
        _editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                if *name == b"img" {
                    self.images += 1;
                }
                if matches!(*name, b"object" | b"script" | b"style" | b"svg") {
                    self.skipped += 1;
                }
            }
            CallbackEvent::EndTag { name }
                if matches!(
                    *name,
                    b"object" | b"script" | b"style" | b"svg"
                ) =>
            {
                self.skipped = self.skipped.saturating_sub(1);
            }
            CallbackEvent::String { value } if self.skipped == 0 => {
                self.text.push_str(&String::from_utf8_lossy(value));
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns Material's rounded read time in minutes.
pub fn calculate(input: &str, words_per_minute: usize) -> usize {
    static SEPARATOR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\W+").expect("static expression"));
    let mut readtime = Readtime::default();
    let _ = html::scan(input, &mut [&mut readtime]);
    let words = SEPARATOR.split(&readtime.text).count();
    let mut seconds = words.saturating_mul(60).div_ceil(words_per_minute);
    let mut penalty = 12;
    for _ in 0..readtime.images {
        seconds += penalty;
        if penalty > 3 {
            penalty -= 1;
        }
    }
    seconds.div_ceil(60)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::calculate;

    #[test]
    fn ignores_embedded_code_and_applies_decreasing_image_penalties() {
        let words = std::iter::repeat_n("word", 265)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(calculate(&format!("<p>{words}</p>"), 265), 1);
        assert_eq!(
            calculate(
                &format!(
                    "<p>{words}</p><script>{words}</script><img><img><img>"
                ),
                265,
            ),
            2
        );
    }
}
