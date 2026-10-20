# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to
# deal in the Software without restriction, including without limitation the
# rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
# sell copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:

# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.

# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
# FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
# IN THE SOFTWARE.

"""Preserve Python regular expression semantics for privacy overrides."""

from __future__ import annotations

import re
from fnmatch import fnmatch


def media_links(content: str, expression: str) -> list[tuple[int, int, str]]:
    """Return URL captures with UTF-8 byte offsets for Rust."""
    matches = []
    for match in re.finditer(expression, content, re.IGNORECASE | re.MULTILINE):
        start, end = match.span("url")
        if start >= 0:
            matches.append(
                (
                    len(content[:start].encode()),
                    len(content[:end].encode()),
                    match.group("url"),
                )
            )
    return matches


def matches(path: str, patterns: list[str]) -> bool:
    """Match asset paths with Material's platform-specific fnmatch rules."""
    return any(fnmatch(path, pattern) for pattern in patterns)
