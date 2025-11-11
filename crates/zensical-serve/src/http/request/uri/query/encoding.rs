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

//! Encoding.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet};
use std::borrow::Cow;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Character set to be percent-encoded.
#[rustfmt::skip]
const SET: &AsciiSet = &percent_encoding::CONTROLS
    .add(b' ').add(b'"').add(b'#').add(b'%').add(b'<').add(b'>').add(b'[')
    .add(b']').add(b'^').add(b'`').add(b'{').add(b'|').add(b'}').add(b'=');

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Encodes a string used in a query string.
#[inline]
#[must_use]
pub fn encode(value: &str) -> Cow<'_, str> {
    utf8_percent_encode(value, SET).into()
}

/// Decodes a string used in a query string.
#[inline]
#[must_use]
pub fn decode(value: &str) -> Cow<'_, str> {
    if value.contains('+') {
        percent_decode_str(&value.replace('+', " "))
            .decode_utf8_lossy()
            .into_owned()
            .into()
    } else {
        percent_decode_str(value).decode_utf8_lossy()
    }
}
