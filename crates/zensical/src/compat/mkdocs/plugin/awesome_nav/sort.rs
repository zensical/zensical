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

//! Awesome-nav sorting compatible with upstream's natsort settings.

use std::cmp::Ordering;

use super::config::{Direction, Sections, SortBy, SortKind};

/// Effective inherited sorting settings.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub by: SortBy,
    pub direction: Direction,
    pub kind: SortKind,
    pub sections: Sections,
    pub ignore_case: bool,
}

/// Sort facts exposed by one resolved navigation item.
pub trait Item {
    fn path(&self) -> &str;
    fn sort_title(&self) -> &str;
    fn is_section(&self) -> bool;
}

/// Sorts items while retaining stable section grouping.
pub fn apply<T: Item>(items: &mut [T], settings: Settings) {
    items.sort_by(|left, right| {
        let ordering = compare(left, right, settings);
        if settings.direction == Direction::Descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    if settings.sections != Sections::Mixed {
        items.sort_by_key(|item| {
            match (settings.sections, item.is_section()) {
                (Sections::First, true) | (Sections::Last, false) => 0,
                _ => 1,
            }
        });
    }
}

fn compare<T: Item>(left: &T, right: &T, settings: Settings) -> Ordering {
    let keys = |item: &T| match settings.by {
        SortBy::Path => vec![item.path().to_owned()],
        SortBy::Filename => {
            vec![file_name(item.path()).into(), item.path().to_owned()]
        }
        SortBy::Title => vec![
            item.sort_title().to_owned(),
            file_name(item.path()).into(),
            item.path().to_owned(),
        ],
    };
    keys(left)
        .into_iter()
        .zip(keys(right))
        .map(|(left, right)| match settings.kind {
            SortKind::Natural if settings.by == SortBy::Title => {
                natural(&left, &right, settings.ignore_case)
            }
            SortKind::Natural => {
                natural_path(&left, &right, settings.ignore_case)
            }
            SortKind::Alphabetical => {
                alphabetical(&left, &right, settings.ignore_case)
            }
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn alphabetical(left: &str, right: &str, ignore_case: bool) -> Ordering {
    if ignore_case {
        left.to_lowercase().cmp(&right.to_lowercase())
    } else {
        left.cmp(right)
    }
}

fn natural(left: &str, right: &str, ignore_case: bool) -> Ordering {
    let mut left = chunks(left);
    let mut right = chunks(right);
    loop {
        match (left.next(), right.next()) {
            (Some(Chunk::Number(left)), Some(Chunk::Number(right))) => {
                let left = left.trim_start_matches('0');
                let right = right.trim_start_matches('0');
                let ordering =
                    left.len().cmp(&right.len()).then_with(|| left.cmp(right));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(Chunk::Text(left)), Some(Chunk::Text(right))) => {
                let ordering = group_letters(left, ignore_case)
                    .cmp(&group_letters(right, ignore_case));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(Chunk::Number(_)), Some(Chunk::Text(_))) => {
                return Ordering::Less;
            }
            (Some(Chunk::Text(_)), Some(Chunk::Number(_))) => {
                return Ordering::Greater;
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

fn natural_path(left: &str, right: &str, ignore_case: bool) -> Ordering {
    let left = path_parts(left);
    let right = path_parts(right);
    for (left, right) in left.iter().zip(&right) {
        let ordering = natural(left, right, ignore_case);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn path_parts(path: &str) -> Vec<&str> {
    let mut result = Vec::new();
    for component in path.split('/') {
        if let Some(index) = component.rfind('.')
            && index > 0
        {
            result.push(&component[..index]);
            result.push(&component[index..]);
        } else {
            result.push(component);
        }
    }
    result
}

fn group_letters(value: &str, ignore_case: bool) -> String {
    let mut output = String::with_capacity(value.len() * 2);
    for character in value.chars() {
        let value = if ignore_case {
            character.to_lowercase().collect::<String>()
        } else {
            character.to_string()
        };
        output.extend(value.chars().flat_map(char::to_lowercase));
        output.push_str(&value);
    }
    output
}

enum Chunk<'a> {
    Number(&'a str),
    Text(&'a str),
}

struct Chunks<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor == self.source.len() {
            return None;
        }
        let numeric = self.source[self.cursor..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit());
        let start = self.cursor;
        for (offset, character) in self.source[start..].char_indices() {
            if character.is_ascii_digit() != numeric {
                self.cursor = start + offset;
                let value = &self.source[start..self.cursor];
                return Some(if numeric {
                    Chunk::Number(value)
                } else {
                    Chunk::Text(value)
                });
            }
        }
        self.cursor = self.source.len();
        let value = &self.source[start..];
        Some(if numeric {
            Chunk::Number(value)
        } else {
            Chunk::Text(value)
        })
    }
}

fn chunks(source: &str) -> Chunks<'_> {
    Chunks { source, cursor: 0 }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::{alphabetical, natural, natural_path};
    use std::cmp::Ordering;

    #[test]
    fn natural_sort_orders_numeric_runs_and_grouped_case() {
        assert_eq!(
            natural_path("page2.md", "page10.md", false),
            Ordering::Less
        );
        assert_eq!(natural("8", "9", false), Ordering::Less);
        assert_eq!(natural("09", "9", false), Ordering::Equal);
        assert_eq!(natural_path("2.md", "2-suffix.md", false), Ordering::Less);
        assert_eq!(natural("A", "a", false), Ordering::Less);
        assert_eq!(natural("a", "B", false), Ordering::Less);
        assert_eq!(
            alphabetical("page2.md", "page10.md", false),
            Ordering::Greater
        );
    }
}
