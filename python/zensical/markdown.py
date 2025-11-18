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

import re
import yaml

from datetime import date, datetime
from markdown import Markdown
from yaml import SafeLoader

from .config import get_config
from .extensions.links import LinksExtension
from .extensions.search import SearchExtension

# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------


FRONT_MATTER_RE = re.compile(
    r"^-{3}[ \r\t]*?\n(.*?\r?\n)(?:\.{3}|-{3})[ \r\t]*\n",
    re.UNICODE | re.DOTALL,
)
"""
Regex pattern to extract front matter.
"""

# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def render(content: str, path: str) -> dict:
    """
    Render Markdown and return HTML.

    This function returns rendered HTML as well as the table of contents and
    metadata. Now, this is the part where Zensical needs to call into Python,
    in order to support the specific syntax of Python Markdown. We're working
    on moving the entire rendering chain to Rust.
    """
    config = get_config()

    # Initialize Markdown parser
    md = Markdown(
        extensions=config["markdown_extensions"],
        extension_configs=config["mdx_configs"],
    )

    # Register links extension, which is equivalent to MkDocs' path resolution
    # Markdown extension. This is a bandaid, until we move this to Rust
    links = LinksExtension(
        use_directory_urls=config["use_directory_urls"], path=path
    )
    links.extendMarkdown(md)

    # Register search extension, which extracts text for search indexing
    search = SearchExtension()
    search.extendMarkdown(md)

    # First, extract metadata - the Python Markdown parser brings a metadata
    # extension, but the implementation is broken, as it does not support full
    # YAML syntax, e.g. lists. Thus, we just parse the metadata with YAML.
    meta = {}
    if match := FRONT_MATTER_RE.match(content):
        try:
            meta = yaml.load(match.group(1), SafeLoader)
            if isinstance(meta, dict):
                content = content[match.end() :].lstrip("\n")
            else:
                meta = {}
        except Exception:
            pass

    # Convert Markdown and set nullish metadata to empty string, since we
    # currently don't have a null value for metadata in the Rust runtime
    content = md.convert(content)
    for key, value in meta.items():
        if value is None:
            meta[key] = ""

        # Convert datetime back to ISO format (for now)
        if isinstance(value, (date, datetime)):
            meta[key] = value.isoformat()

    # Obtain search index data, unless page is excluded
    search = md.postprocessors["search"]
    if meta.get("search", {}).get("exclude", False):
        search.data = []

    # Return Markdown with metadata
    return {
        "meta": meta,
        "content": content,
        "search": search.data,
        "title": "",
        "toc": [_convert_toc(item) for item in getattr(md, "toc_tokens", [])],
    }


def _convert_toc(item: any):
    """
    Convert a table of contents item to navigation item format.
    """
    toc_item = {
        "title": item["data-toc-label"] or item["name"],
        "id": item["id"],
        "url": f"#{item['id']}",
        "children": [],
        "level": item["level"],
    }

    # Recursively convert items
    for child in item["children"]:
        toc_item["children"].append(_convert_toc(child))

    # Return table of contents item
    return toc_item
