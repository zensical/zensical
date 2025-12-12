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

import codecs
import functools
import os
from glob import iglob
from typing import TYPE_CHECKING
from xml.etree.ElementTree import Element

from pymdownx import emoji, twemoji_db

if TYPE_CHECKING:
    from markdown import Markdown

# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def twemoji(options: dict, md: Markdown) -> dict:  # noqa: ARG001
    """Create twemoji index."""
    paths = options.get("custom_icons", [])[:]
    return _load_twemoji_index(tuple(paths))


def to_svg(
    index: str,
    shortname: str,
    alias: str,
    uc: str | None,
    alt: str,
    title: str,
    category: str,
    options: dict,
    md: Markdown,
) -> Element[str]:
    """Load icon."""
    if not uc:
        icons = md.inlinePatterns["emoji"].emoji_index["emoji"]  # type: ignore[attr-defined]

        # Create and return element to host icon
        el = Element("span", {"class": options.get("classes", index)})
        el.text = md.htmlStash.store(_load(icons[shortname]["path"]))
        return el

    # Delegate to `pymdownx.emoji` extension
    return emoji.to_svg(
        index, shortname, alias, uc, alt, title, category, options, md
    )


# -----------------------------------------------------------------------------
# Helper functions
# -----------------------------------------------------------------------------


@functools.cache
def _load(file: str) -> str:
    """Load icon from file."""
    with codecs.open(file, encoding="utf-8") as f:
        return f.read()


@functools.cache
def _load_twemoji_index(paths: tuple[str, ...]) -> dict:
    """Load twemoji index and add icons."""
    index = {
        "name": "twemoji",
        "emoji": twemoji_db.emoji,
        "aliases": twemoji_db.aliases,
    }

    # Compute path to theme root and traverse all icon directories
    root = os.path.dirname(os.path.dirname(__file__))
    root = os.path.join(root, "templates", ".icons")
    for path in [*paths, root]:
        base = os.path.normpath(path)

        # Index icons provided by the theme and via custom icons
        glob = os.path.join(base, "**", "*.svg")
        svgs = iglob(os.path.normpath(glob), recursive=True)
        for file in svgs:
            icon = file[len(base) + 1 : -4].replace(os.path.sep, "-")

            # Add icon to index
            name = f":{icon}:"
            if not any(name in index[key] for key in ["emoji", "aliases"]):
                index["emoji"][name] = {"name": name, "path": file}

    # Return index
    return index
