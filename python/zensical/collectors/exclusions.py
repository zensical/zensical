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
    # Escaped characters
    (?P<escaped>
        \\`                         # Backslash followed by any character
    )
    |
    # Fenced code blocks
    (?P<fenced>
        ^(?P<indent>[ \t]*)         # Capture leading indentation
        (?P<fence>`{3,}|~{3,})      # Capture fence character and length
        [^\r\n]*\r?\n               # Optional info string
        .*?                         # Block content
        ^[ \t]*(?P=fence)[`~]*      # Closing fence: same type, at least as long
        [^\r\n]*(\r?\n|$)           # Optional trailing content
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
        (?P<attrs>[ \t][^>]*)?      # Optional attributes (captured)
        >[ \t]*\r?\n                # Close of tag, end of line
        (?:(?!^<(?P=tag)[\s>]).)*?  # Block content, stop before same tag
        ^</(?P=tag)>[ \t]*$         # Closing tag
    )
    |
    # Block math:
    #
    #   $$
    #   ...
    #   $$
    #
    (?P<math_block>
        ^[ \t]*\$\$[ \t]*\r?\n      # Opening $$ on its own line
        .*?                         # Math content
        ^[ \t]*\$\$[ \t]*$          # Closing $$ on its own line
    )
    |
    # Block math (alternate):
    #
    #   \[
    #   ...
    #   \]
    #
    (?P<math_block_alt>
        ^[ \t]*\\\[[ \t]*\r?\n      # Opening \[ on its own line
        .*?                         # Math content
        ^[ \t]*\\\][ \t]*$          # Closing \] on its own line
    )
    |
    # Inline math:
    #
    #   $f(x)$
    #
    (?P<math_inline>
        (?<!\$)\$(?![\s$])          # Opening $, not preceded/followed by $
        (?-s:.*?)                   # Math content (same line only)
        (?<![\s$])\$(?!\$)          # Closing $, not preceded by space or $
    )
    |
    # Inline math (alternate):
    #
    #   \(f(x)\)
    #
    (?P<math_inline_alt>
        \\\(                        # Opening \(
        (?-s:.*?)                   # Math content (same line only)
        \\\)                        # Closing \)
    )
    |
    # Inline code blocks
    (?P<inline>
        (?P<ticks>`+)               # Opening backticks
        .+?                         # Block content
        (?P=ticks)                  # Closing backticks (matching)
    )
    """,
    re.VERBOSE | re.MULTILINE | re.DOTALL,
)
"""
Match regions that should be excluded from reference scanning.

This includes fenced code blocks, inline code blocks, HTML comments, and HTML
blocks without `markdown` attribute, as they may contain link-like patterns that
should not be treated as references.
"""

# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def exclusions(content: bytes, shift: int = 0) -> Iterator[Span]:
    """Scan Markdown and yield exclusions."""
    for match in _RE.finditer(content):
        if match.lastgroup != "html":
            yield Span(shift + match.start(), shift + match.end())
            continue

        # In case this is a non-Markdown HTML block, we exclude the entire
        # block, as it may contain link-like patterns that must be ignored
        attrs = match.group("attrs") or b""
        if b"markdown" not in attrs:
            yield Span(shift + match.start(), shift + match.end())
            continue

        # Exclude opening tag line
        end = content.index(b"\n", match.start()) + 1
        yield Span(shift + match.start(), shift + end)

        # Exclude closing tag line (cut from block), and recurse
        start = end
        end = content.rindex(b"\n", 0, match.end())
        yield from exclusions(content[start:end], shift + start)
