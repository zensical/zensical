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

import json
import re
from datetime import date, datetime
from typing import Any

from markdown import Markdown

from zensical.config import get_config
from zensical.extensions.autorefs import set_autorefs_page
from zensical.extensions.context import ContextExtension, Page
from zensical.extensions.links import LinksExtension


def render(content: str, path: str, url: str, metadata: str = "{}") -> dict:
    """Render Markdown and return HTML.

    This function returns rendered HTML as well as the table of contents and
    metadata. Now, this is the part where Zensical needs to call into Python,
    in order to support the specific syntax of Python Markdown. We're working
    on moving the entire rendering chain to Rust.
    """
    # Metadata inheritance and front matter are resolved in Rust before this
    # boundary. JSON keeps the call explicit and avoids reconstructing Python
    # objects one value at a time through the FFI.
    meta: dict = json.loads(metadata)

    # Create page context and set it for autorefs.
    # We can stop setting the page if/when we vendor mkdocstrings.
    page = Page(url=url, path=path, meta=meta)
    set_autorefs_page(page)

    # Update configuration to include context extension.
    # It's important we mutate the global configuration here,
    # to allow mkdocstrings and markdown-exec to forward
    # the extension to their inner Markdown instances.
    config = get_config()
    for extension in config["markdown_extensions"]:
        if isinstance(extension, ContextExtension):
            extension._kwargs["page"] = page
            break
    else:
        config["markdown_extensions"].insert(
            0,
            ContextExtension(
                page=page,
                config=config,
            ),
        )

    # Initialize Markdown parser
    md = Markdown(
        extensions=config["markdown_extensions"],
        extension_configs=config["mdx_configs"],
    )

    # Note: mkdocstrings and markdown-exec do not need to propagate the links
    # extension to their inner Markdown instances. Its postprocessor runs last
    # and can see inner layer contents. More importantly, inner layers *must
    # not* run the extension: the inner treeprocessor would transform links
    # once, and the outer postprocessor would transform them again.

    # Register links extension, which is equivalent to MkDocs' path resolution
    # Markdown extension. This is a bandaid, until we move this to Rust
    links = LinksExtension(
        use_directory_urls=config["use_directory_urls"], path=path
    )
    links.extendMarkdown(md)

    # Inform markdown-exec that it runs through Zensical.
    try:
        import markdown_exec  # noqa: PLC0415  # ty:ignore[unresolved-import]
    except ImportError:
        pass
    else:
        markdown_exec._caller = "zensical"

    # Convert content to HTML
    content = md.convert(content)

    # Sanitize metadata before passing it to Rust
    meta = {k: _sanitize(v) for k, v in meta.items()}

    # Return Markdown with metadata
    return {
        "meta": meta,
        "title": "",
        "content": content,
        "toc": [_convert_toc(item) for item in getattr(md, "toc_tokens", [])],
    }


def _sanitize(value: Any) -> Any:
    if isinstance(value, (date, datetime)):
        return value.isoformat()
    if isinstance(value, dict):
        return {k: _sanitize(x) for k, x in value.items()}
    if isinstance(value, list):
        return [_sanitize(x) for x in value]
    return value


def _convert_toc(item: Any) -> dict:
    """Convert a table of contents item to navigation item format."""
    toc_item = {
        "title": item["data-toc-label"] or item["name"],
        "content": item["data-toc-label"] or _cleanup_toc_label(item["html"]),
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


def _cleanup_toc_label(html: str) -> str:
    """Clean up a TOC label."""
    # Remove links
    html = re.sub(r"id=\"?[^\">]+\"?", "", html)
    html = re.sub(r"<a\s+[^>]+>(.*?)</a>", r"\1", html, flags=re.DOTALL)
    # Remove abbreviations
    html = re.sub(r"<abbr\s+[^>]+>(.*?)</abbr>", r"\1", html, flags=re.DOTALL)
    # Remove images
    html = re.sub(r"<img\s+[^>]+>", "", html, flags=re.DOTALL)
    return html  # noqa: RET504
