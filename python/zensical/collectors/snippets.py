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
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from zensical.utilities.span import Span

if TYPE_CHECKING:
    from collections.abc import Iterator
    from re import Match

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


@dataclass
class LineRange:
    """A line range selection within a file.

    Both `start` and `end` are 1-based line numbers. Either may be `None` to
    indicate an open-ended range:

    - `LineRange(3, None)` - from line 3 to end of file  (`:3`)
    - `LineRange(None, 3)` - from start of file to line 3 (`::3`)
    - `LineRange(4, 6)`    - lines 4 to 6  (`:4:6`)
    """

    start: int | None
    """Starting line number, or `None` for start of file."""

    end: int | None
    """Ending line number, or `None` for end of file."""

    def __repr__(self) -> str:
        start = self.start or ""
        end = f":{self.end}" if self.end is not None else ""
        return f":{start}{end}"


@dataclass
class Snippet(Span):
    """A snippet inclusion site, e.g. `--8<-- "file.md"`."""

    path: Span
    """Span of the file path, without quotes, line range, or section suffix."""

    section: str | None = None
    """Named section to extract, e.g. `func` from `file.md:func`."""

    lines: list[LineRange] = field(default_factory=list)
    """Line range selections, e.g. `[LineRange(1,3), LineRange(5,6)]` from
    `file.md:1:3,5:6`. Empty list means include the whole file."""


# -----------------------------------------------------------------------------
# Constants
# -----------------------------------------------------------------------------

_SINGLE_RE = re.compile(
    r"""
    ^[^\S\n]*-+8<-+[^\S\n]+"   # Scissors marker + opening quote
    (?!;)(?!https?://)         # Skip comments and URLs
    (?P<path>[^":]+?)          # File path
    (?P<suffix>[:#][^"]*)?     # Optional :line, :section, or #anchor
    "
    """,
    re.VERBOSE,
)
"""Match the single-line format: --8<-- "file.md" or --8<-- "file.md:4:6"."""

_BLOCK_RE = re.compile(r"^[^\S\n]*-+8<-+[^\S\n]*$")
"""Match the block format opener and closer: --8<-- on a line by itself."""

_ENTRY_RE = re.compile(
    r"""
    ^(?P<indent>[^\S\n]*)      # Optional leading indentation
    (?!;)                      # Not a commented-out entry
    (?!https?://)              # Not a URL entry
    (?P<path>[^:#\s]+?)        # File path
    (?P<suffix>[:#]\S*)?       # Optional :line, :section, or #anchor
    [^\S\n]*$                  # End of line
    """,
    re.VERBOSE,
)
"""Match a single entry line inside a block - skip comments and URLs."""

_RANGES_RE = re.compile(r"^[\d:,\-]*$")
"""Match a suffix that consists only of line range characters."""

# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def snippets(lines: list[str]) -> Iterator[Snippet]:
    r"""Scan Markdown and yield all snippets."""
    offset = 0
    in_block = False

    # Iterate over lines with their start offsets to compute absolute positions
    # for snippet spans, and keep track of whether we're inside a block format
    for line in lines:
        shift, offset = offset, offset + len(line)
        value = line.rstrip("\r\n")

        # We're inside a block
        if _BLOCK_RE.match(value):
            in_block = not in_block
            continue

        # Select the appropriate regex based on whether we're in a block or not
        match = (_ENTRY_RE if in_block else _SINGLE_RE).match(value)
        if match:
            range = slice(shift, shift + len(value))
            path = _span(shift, match, "path")

            # Parse the optional suffix for section and line range information
            section, ranges = _parse_suffix(match.group("suffix"))
            yield Snippet(value, range, path, section, ranges)


# ----------------------------------------------------------------------------


def _span(shift: int, match: Match[str], name: str) -> Span:
    """Build a span for a named capture group within a match."""
    start, end = match.start(name), match.end(name)
    return Span(match.group(name), slice(shift + start, shift + end))


def _parse_suffix(suffix: str | None) -> tuple[str | None, list[LineRange]]:
    """Parse the optional suffix after the file path."""
    if not suffix:
        return None, []

    # Strip the leading : or # and check if it's a line range
    body = suffix.lstrip(":#")
    if not _RANGES_RE.match(body):
        return body, []

    # Parse a single line range part, e.g. "1:3" or ":4"
    def _range(part: str) -> LineRange:
        start, _, end = part.partition(":")
        return LineRange(
            int(start) if start else None, int(end) if end else None
        )

    # Parse comma-separated line range parts, e.g. "1:3,5:6"
    return None, [_range(p) for p in body.split(",")]
