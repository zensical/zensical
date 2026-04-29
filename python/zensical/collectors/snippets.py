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
class SnippetRange:
    """A range selection within a file.

    Both `start` and `end` are 1-based line numbers. Either may be `None` to
    indicate an open-ended range:

    - `SnippetRange(3, None)` - from line 3 to end of file (`:3`)
    - `SnippetRange(None, 3)` - from start of file to line 3 (`::3`)
    - `SnippetRange(4, 6)`    - lines 4 to 6 (`:4:6`)
    """

    start: int | None
    """Starting line number, or `None` for start of file."""

    end: int | None
    """Ending line number, or `None` for end of file."""

    def __repr__(self) -> str:
        start = self.start or ""
        end = f":{self.end}" if self.end is not None else ""
        return f":{start}{end}"


# ----------------------------------------------------------------------------


@dataclass
class SnippetFile(Span):
    """A file reference within a snippet."""

    anchor: Span | None = None
    """Span of the named anchor, e.g. `object` from `file.md:object`."""

    ranges: list[SnippetRange] = field(default_factory=list)
    """Ranges. Empty list means include the whole file."""


@dataclass
class Snippet(Span):
    """A snippet marker, e.g. `--8<-- "file.md"`."""

    indent: int
    """Number of leading bytes of indentation on the marker line."""

    files: list[SnippetFile]
    """File references contained in this snippet."""


# -----------------------------------------------------------------------------
# Constants
# -----------------------------------------------------------------------------

_SINGLE_RE = re.compile(
    rb"""
    ^[^\S\n]*-+8<-+[^\S\n]+"   # Scissors marker + opening quote
    (?!;)(?!https?://)         # Skip comments and URLs
    (?P<file>[^:]+?)           # File path
    (?P<suffix>[:][^"]*)?      # Optional :ranges, :anchor
    "
    """,
    re.VERBOSE,
)
"""Match the single-line marker: --8<-- "file.md" or --8<-- "file.md:4:6"."""

_BLOCK_RE = re.compile(rb"^[^\S\n]*-+8<-+[^\S\n]*$")
"""Match the block opening and closing marker: --8<-- on a line by itself."""

_BLOCK_ENTRY_RE = re.compile(
    rb"""
    ^[^\S\n]*                  # Optional leading indentation
    (?!;)                      # Not a commented-out entry
    (?!https?://)              # Not a URL entry
    (?P<file>[^:\s]+?)         # File path
    (?P<suffix>[:]\S*)?        # Optional :ranges, :anchor
    [^\S\n]*$                  # End of line
    """,
    re.VERBOSE,
)
"""Match a single entry line inside a block."""

_RANGES_RE = re.compile(rb"^[\d:,\-]*$")
"""Match a suffix that consists only of line range characters."""

# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def snippets(markdown: bytes) -> Iterator[Snippet]:
    r"""Scan Markdown and yield all snippets."""
    lines = iter(markdown.splitlines(keepends=True))
    index = 0

    # Iterate over lines until we find a snippet marker, then consume either a
    # block of entries or a single line, and yield snippet and files references
    for line in lines:
        shift, index = index, index + len(line)
        value = line.rstrip(b"\r\n")

        # Encountered block opening marker
        if _BLOCK_RE.match(value):
            files: list[SnippetFile] = []
            start = shift

            # Determine indentation level of block entries based on first line
            # after marker, and collect entries until closing marker is found
            indent = len(value) - len(value.lstrip(b" \t"))
            for line in lines:  # noqa: PLW2901
                shift, index = index, index + len(line)
                value = line.rstrip(b"\r\n")

                # Encountered block closing marker
                if _BLOCK_RE.match(value):
                    end = shift + len(value)
                    yield Snippet(start, end, indent, files)
                    break

                # We're not at the closing marker yet, so try to match an entry
                # line, and if it matches, add it to the list of files
                match = _BLOCK_ENTRY_RE.match(value)
                if match:
                    files.append(_file(shift, match))

            # Continue on next line
            continue

        # Check for single-line marker
        match = _SINGLE_RE.match(value)
        if match:
            start, end = shift + match.start(), shift + match.end()

            # Determine indentation level of marker line, which will be used
            # for indentation of any content that will be inserted
            indent = len(value) - len(value.lstrip(b" \t"))
            yield Snippet(start, end, indent, [_file(shift, match)])


# ----------------------------------------------------------------------------


def _file(shift: int, match: Match[bytes]) -> SnippetFile:
    """Build a snippet file from a match."""
    start, end = match.start("file"), match.end("file")
    suffix = match.group("suffix")

    # Determine whether suffix is a named anchor or line ranges
    body = suffix.lstrip(b":#") if suffix else None
    if body and not _RANGES_RE.match(body):
        offset = shift + match.start("suffix") + (len(suffix) - len(body))
        anchor = Span(offset, offset + len(body))
        ranges = []
    else:
        anchor = None
        ranges = _ranges(body)

    # Return the file reference with its anchor and ranges, if any
    return SnippetFile(shift + start, shift + end, anchor, ranges)


def _ranges(body: bytes | None) -> list[SnippetRange]:
    """Parse comma-separated line range selections from a suffix body."""
    if not body:
        return []

    def _range(part: bytes) -> SnippetRange:
        start, _, end = part.partition(b":")
        return SnippetRange(
            int(start) if start else None,
            int(end) if end else None,
        )

    return [_range(p) for p in body.split(b",")]
