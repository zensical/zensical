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

from typing import TYPE_CHECKING

import pytest
from markdown import Markdown

from zensical.extensions.links import (
    LinksExtension,
    LinksPostprocessor,
    _is_relative,
    _md_path_to_html,
    _rewrite_url,
)

if TYPE_CHECKING:
    from collections.abc import Iterator
    from typing import Any


class _TrackingBlocks(list[str]):
    """Track indexes read from stashed blocks."""

    def __init__(self, values: list[str]) -> None:
        super().__init__(values)
        self.visited: list[int] = []

    def __iter__(self) -> Iterator[str]:
        for index, value in enumerate(super().__iter__()):
            self.visited.append(index)
            yield value

    def __getitem__(self, index: Any) -> Any:
        if isinstance(index, int):
            self.visited.append(index)
        return super().__getitem__(index)


class _TrackingPostprocessor(LinksPostprocessor):
    """Record which stash indexes each invocation reads."""

    def __init__(self, md: Markdown, blocks: _TrackingBlocks) -> None:
        super().__init__(md, "guide/page.md", True)
        self._blocks = blocks
        self.visits: list[list[int]] = []

    def run(self, text: str) -> str:
        start = len(self._blocks.visited)
        text = super().run(text)
        self.visits.append(self._blocks.visited[start:])
        return text


@pytest.mark.parametrize(
    ("path", "directory_urls", "expected"),
    [
        ("index.md", True, ""),
        ("README.md", True, ""),
        ("guide/README.md", True, "guide/"),
        ("guide/page.md", True, "guide/page/"),
        ("myindex.md", True, "myindex/"),
        ("guide/README.md", False, "guide/index.html"),
        ("guide/page.md", False, "guide/page.html"),
        ("assets/app.js", True, "assets/app.js"),
    ],
)
def test_markdown_path_routing(
    path: str, directory_urls: bool, expected: str
) -> None:
    """Markdown links retain the current MkDocs-compatible route shape."""
    assert _md_path_to_html(path, directory_urls) == expected


@pytest.mark.parametrize(
    "value",
    ["https://example.com", "//example.com", "/root", "#section"],
)
def test_non_relative_references_are_not_rewritten(value: str) -> None:
    """External, root-relative, and same-page references remain untouched."""
    assert not _is_relative(value)
    assert _rewrite_url(value, "guide/page.md", True) is None


def test_rewrite_preserves_query_and_fragment() -> None:
    """Only the path component changes when a Markdown URL is rewritten."""
    assert (
        _rewrite_url("other.md?view=full#details", "guide/page.md", True)
        == "../other/?view=full#details"
    )


def test_rewrites_markdown_links_after_inline_processing() -> None:
    """The treeprocessor must run after `inline` creates link elements."""
    md = Markdown(
        extensions=[
            LinksExtension(path="guide/index.md", use_directory_urls=True)
        ]
    )

    assert md.convert("[Guide](guide.md)") == (
        '<p><a href="guide/">Guide</a></p>'
    )


def test_rewrites_markdown_links_after_unescaping_url() -> None:
    """The treeprocessor must see URLs after core `unescape` runs."""
    md = Markdown(
        extensions=[
            LinksExtension(path="guide/index.md", use_directory_urls=True)
        ]
    )

    assert md.convert(r"[Guide](guide\.md)") == (
        '<p><a href="guide/">Guide</a></p>'
    )


def test_rewrites_links_in_stashed_raw_html() -> None:
    """Raw HTML must be updated before Python-Markdown restores its stash."""
    md = Markdown(
        extensions=[
            LinksExtension(path="index.md", use_directory_urls=True),
        ],
    )

    assert md.convert('<div><a href="guide.md">Guide</a></div>') == (
        '<div><a href="guide/">Guide</a></div>'
    )


def test_postprocessor_does_not_rescan_stash_for_toc() -> None:
    """TOC rendering does not cause processed blocks to be scanned again."""
    md = Markdown(extensions=["toc"])
    blocks = _TrackingBlocks([])
    md.htmlStash.rawHtmlBlocks = blocks
    processor = _TrackingPostprocessor(md, blocks)
    md.postprocessors.register(processor, processor.name, 29)

    html = md.convert(
        '# One\n\n## Two\n\n## Three\n\n<a href="other.md">other</a>'
    )

    assert len(processor.visits) > 1
    assert processor.visits[0] == list(range(len(blocks)))
    assert all(not visited for visited in processor.visits[1:])
    assert '<a href="../other/">other</a>' in html
