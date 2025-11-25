# Copyright (c) 2025 Zensical and contributors

# SPDX-License-Identifier: MIT
# Third-party contributions licensed under DCO

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

from markdown import Extension, Markdown
from markdown.treeprocessors import Treeprocessor
from markdown.util import AMP_SUBSTITUTE
from pathlib import PurePosixPath
from xml.etree.ElementTree import Element
from urllib.parse import urlparse

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class LinksProcessor(Treeprocessor):
    """
    Tree processor to replace links in Markdown with URLs.

    Note that we view this as a bandaid until we can do processing on proper
    HTML ASTs in Rust. In the meantime, we just replace them as we find them.
    This processor will replace links to other Markdown files, as well as
    adjust asset links if directory URLs are used.
    """

    def __init__(self, md: Markdown, path: str, use_directory_urls: bool):
        super().__init__(md)
        self.path = path  # Current page
        self.use_directory_urls = use_directory_urls

    def run(self, root: Element):
        # Now, we determine whether the current page is an index page, as we
        # must apply slightly different handling in case of directory URLs
        current_is_index = get_name(self.path) in ("index.md", "README.md")
        for el in root.iter():
            # In case the element has a `href` or `src` attribute, we parse it
            # as an URL, so we can analyze and alter its path
            key = next((k for k in ("href", "src") if el.get(k)), None)
            if not key:
                continue

            # Extract value - Python Markdown does some weird stuff where it
            # replaces mailto: links with double encoded entities. MkDocs just
            # skips if it detects that, so we do the same.
            value = el.get(key)
            if AMP_SUBSTITUTE in value:
                continue

            # Parse URL and skip everything that is not a relative link
            url = urlparse(value)
            if url.scheme or url.netloc:
                continue

            # Leave anchors that go to the same page as they are
            if not url.path and url.fragment:
                continue

            # Now, adjust relative links to Markdown files
            path = url.path
            if path.endswith(".md"):
                path = path.removesuffix(".md") + ".html"
                if self.use_directory_urls:
                    name = get_name(path)
                    if name in ("index.html", "README.html"):
                        path = path[: -len(name)]
                    elif path.endswith(".html"):
                        path = path[: -len(".html")] + "/"

            # If the current page is not an index page, and we should render
            # directory URLs, we need to prepend a "../" to all links
            if not current_is_index and self.use_directory_urls:
                path = f"../{path}"

            # Reassemble URL and update link
            el.set(key, url._replace(path=path).geturl())


# -----------------------------------------------------------------------------


class LinksExtension(Extension):
    """
    A Markdown extension to resolve links to other Markdown files.
    """

    def __init__(self, path: str, use_directory_urls: bool):
        """
        Initialize the extension.
        """
        self.path = path  # Current page
        self.use_directory_urls = use_directory_urls

    def extendMarkdown(self, md: Markdown):
        """
        Register Markdown extension.
        """
        md.registerExtension(self)

        # Create and register treeprocessor - we use the same priority as the
        # `relpath` treeprocessor, the latter of which is guaranteed to run
        # after our treeprocessor, so we can check the original Markdown URIs
        # before they are resolved to URLs.
        processor = LinksProcessor(md, self.path, self.use_directory_urls)
        md.treeprocessors.register(processor, "zrelpath", 0)


# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def get_name(path: str) -> str:
    """
    Get the name of a file from a given path.
    """
    path = PurePosixPath(path)
    return path.name
