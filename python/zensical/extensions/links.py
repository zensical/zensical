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
from pathlib import PurePosixPath
from typing import TYPE_CHECKING
from urllib.parse import urlparse

from markdown.extensions import Extension
from markdown.postprocessors import Postprocessor
from markdown.treeprocessors import Treeprocessor
from markdown.util import AMP_SUBSTITUTE

if TYPE_CHECKING:
    from xml.etree.ElementTree import Element

    from markdown import Markdown

# -----------------------------------------------------------------------------
# Constants
# -----------------------------------------------------------------------------

_RE = re.compile(
    r'(?:href|src)=(?P<quote>["\'])(?P<value>[^"\']+)(?P=quote)',
    re.IGNORECASE,
)
"""Match `href` and `src` attribute values in stashed raw HTML blocks."""

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class LinksTreeprocessor(Treeprocessor):
    """Rewrites relative links."""

    def __init__(self, md: Markdown, path: str, use_directory_urls: bool):
        super().__init__(md)
        self.path = path
        self.use_directory_urls = use_directory_urls

    def run(self, root: Element) -> None:
        """Walk the element tree and rewrites `href` and `src` attributes."""
        for el in root.iter():
            # In case the element has a `href` or `src` attribute, we parse it
            # as an URL, so we can analyze and alter its path
            key = next((k for k in ("href", "src") if el.get(k)), None)
            if not key:
                continue

            # Rewrite relative links, leaving absolute URLs unchanged
            if url := _rewrite_url(
                el.get(key, ""), self.path, self.use_directory_urls
            ):
                el.set(key, url)


class LinksPostprocessor(Postprocessor):
    """Rewrites relative links in stashed raw HTML blocks.

    This postprocessor complements the :class:`LinksTreeprocessor` by applying
    the same URL rewriting logic to raw HTML blocks that Python-Markdown stashes
    before tree processing and reinstates afterward. This ensures that links
    inside raw HTML are handled consistently as well.
    """

    def __init__(self, md: Markdown, path: str, use_directory_urls: bool):
        super().__init__(md)
        self._path = path
        self._use_directory_urls = use_directory_urls
        self._processed: set[int] = set()

    def run(self, text: str) -> str:
        """Rewrite `href` and `src` attributes of stashed HTML blocks."""
        for i, raw in enumerate(self.md.htmlStash.rawHtmlBlocks):
            if i not in self._processed:
                self.md.htmlStash.rawHtmlBlocks[i] = _RE.sub(
                    self._maybe_process, raw
                )
                self._processed.add(i)

        # Return text unmodified, as we only need to modify the stashed raw HTML
        # blocks, which will later be reinstated by the raw HTML postprocessor
        return text

    def _maybe_process(self, m: re.Match[str]) -> str:
        """Rewrite a single matched `href` or `src` value."""
        value = m.group("value")

        # Rewrite relative links, leaving absolute URLs unchanged
        updated = _rewrite_url(value, self._path, self._use_directory_urls)
        if updated is None:
            return m.group(0)

        # Reconstruct the attribute with the original quote style preserved
        q = m.group("quote")
        attr = m.group(0).split("=")[0]
        return f"{attr}={q}{updated}{q}"


# -----------------------------------------------------------------------------


class LinksExtension(Extension):
    """Markdown extension to rewrite relative links to other files.

    Registers both a treeprocessor for links in the normal Markdown flow and
    a postprocessor for links inside stashed raw HTML blocks, so that all
    relative URLs are rewritten consistently regardless of how they appear in
    the source document.
    """

    def __init__(self, path: str, use_directory_urls: bool) -> None:
        """Initialize the extension."""
        self.path = path
        self.use_directory_urls = use_directory_urls

    def extendMarkdown(self, md: Markdown) -> None:  # noqa: N802
        """Register Markdown extension."""
        md.registerExtension(self)

        # Register treeprocessor - run before `inline` (priority 20)
        treeprocessor = LinksTreeprocessor(
            md, self.path, self.use_directory_urls
        )
        md.treeprocessors.register(treeprocessor, "zrelpath", 0)

        # Register postprocessor - run before `raw_html` (priority 30)
        postprocessor = LinksPostprocessor(
            md, self.path, self.use_directory_urls
        )
        md.postprocessors.register(postprocessor, "zrelpath", 29)


# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def _get_name(value: str) -> str:
    """Return the filename component of a POSIX-style path."""
    path = PurePosixPath(value)
    return path.name


def _is_relative(value: str) -> bool:
    """Determine whether a URL string is a relative link."""
    if AMP_SUBSTITUTE in value:
        return False

    # Absolute URLs (e.g. `https://example.com`) and protocol-relative URLs
    url = urlparse(value)
    if url.scheme or url.netloc or url.path.startswith("/"):
        return False

    # Anchor-only references (e.g. `#section`) should not be rewritten, as they
    # point to a section within the same page rather than a different page
    return not (not url.path and url.fragment)


def _md_path_to_html(path: str, use_directory_urls: bool) -> str:
    """Convert a relative `.md` path to its final HTML form."""
    if not path.endswith(".md"):
        return path

    # Convert the `.md` extension to `.html` and extract the file name
    path = path.removesuffix(".md") + ".html"
    name = _get_name(path)

    # When directory URLs are enabled, `index.html` and `README.html` collapse
    # to their parent directory, while all other pages become directories with
    # a trailing slash. When directory URLs are disabled, `README.html` is
    # served as `index.html`, while all other pages remain unchanged.
    if use_directory_urls:
        if name in ("index.html", "README.html"):
            return path.removesuffix(name)

        # All other pages become directories (trailing slash)
        return path.removesuffix(".html") + "/"

    # README.html is served as index.html in flat URL mode
    if name == "README.html":
        return path.removesuffix("README.html") + "index.html"

    # No change needed
    return path


def _apply_directory_prefix(
    value: str, path: str, use_directory_urls: bool
) -> str:
    """Prepend `../` for non-index pages when directory URLs are enabled."""
    is_index = _get_name(path) in ("index.md", "README.md")
    if not is_index and use_directory_urls:
        return f"../{value}"

    # No change needed
    return value


def _rewrite_url(value: str, path: str, use_directory_urls: bool) -> str | None:
    """Rewrite a relative URL."""
    if not _is_relative(value):
        return None

    # Parse URL, so we can analyze and alter its path while preserving other
    # components like query parameters and fragments
    url = urlparse(value)

    # Rewrite the path component, noting that the URL may be relative to the
    # current page, so we need to adjust it accordingly
    value = _md_path_to_html(url.path, use_directory_urls)
    value = _apply_directory_prefix(value, path, use_directory_urls)
    return url._replace(path=value).geturl()
