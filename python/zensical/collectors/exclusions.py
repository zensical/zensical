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

from __future__ import annotations

import re
from typing import TYPE_CHECKING

from zensical.utilities.span import Span

if TYPE_CHECKING:
    from collections.abc import Iterator

# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------

_RE = re.compile(
    rb"""
    # Fenced code blocks
    (?P<fenced>
        ^(?P<indent>[^\S\n]*)       # Capture leading indentation
        (?P<fence>`{3,}|~{3,})      # Capture fence character and length
        [^\n]*\n                    # Optional info string
        .*?                         # Block content
        ^(?P=indent)(?P=fence)      # Closing fence must match indent + fence
        [^\n]*$                     # Optional trailing content
    )
    |
    # HTML comments (block and inline)
    (?P<comment>
        <!--                        # Opening delimiter
        .*?                         # Comment content
        -->                         # Closing delimiter
    )
    |
    # HTML blocks
    (?P<html>
        ^<(?P<tag>\w+)              # Opening block-level tag
        (?:[^\S\n][^>]*)?>          # Optional attributes
        .*?                         # Block content
        (?:^</(?P=tag)>[^\n]*)?     # Optional closing tag
        (?=\n\n|\Z)                 # Ends at blank line or end of file
    )
    |
    # Inline code blocks
    (?P<inline>
        (?P<ticks>`+)               # Opening backticks
        .+?                         # Block content
        (?P=ticks)                  # Closing backticks (matching)
    )
    |
    # Task list checkboxes
    (?P<task>
        ^[^\S\n]*[-*+][^\S\n]+\[[xX ]\]
    )
    """,
    re.VERBOSE | re.MULTILINE | re.DOTALL,
)
"""
Match regions that should be excluded from reference scanning.

This includes fenced code blocks, inline code blocks, and task list checkboxes,
which may contain link-like patterns that should not be treated as references.
"""

# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def exclusions(markdown: bytes) -> Iterator[Span]:
    """Scan Markdown and yield exclusions."""
    for match in _RE.finditer(markdown):
        yield Span(match.start(), match.end())
