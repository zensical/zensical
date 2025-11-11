// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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

//! Navigation iterator.

use super::item::NavigationItem;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Navigation iterator.
pub struct Iter<'a> {
    /// Iteration stack.
    stack: Vec<(&'a [NavigationItem], usize)>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<'a> Iter<'a> {
    /// Creates a navigation iterator.
    pub fn new(items: &'a [NavigationItem]) -> Self {
        let mut stack = Vec::new();
        if !items.is_empty() {
            stack.push((items, 0));
        }
        Self { stack }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<'a> Iterator for Iter<'a> {
    type Item = &'a NavigationItem;

    /// Advances the iterator and returns the next item.
    fn next(&mut self) -> Option<Self::Item> {
        while let Some((slice, index)) = self.stack.last_mut() {
            if *index >= slice.len() {
                self.stack.pop();
                continue;
            }

            // Advance index
            let item = &slice[*index];
            *index += 1;

            // Push children slice so they are visited next (pre-order)
            if !item.children.is_empty() {
                self.stack.push((item.children.as_slice(), 0));
            }

            // Return current item
            return Some(item);
        }

        // No more items
        None
    }
}
