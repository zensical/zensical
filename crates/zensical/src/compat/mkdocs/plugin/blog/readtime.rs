// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Native Material-compatible post read-time calculation.

use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use regex::Regex;
use std::sync::LazyLock;

use crate::compat::mkdocs::html::{self, Editor, Visitor};

#[derive(Default)]
struct Readtime {
    text: String,
    images: usize,
    skipped: usize,
}

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
