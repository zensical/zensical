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

from typing import TYPE_CHECKING, Any

from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTS: dict[str, Any] = {"clean": False, "strict": False}


def test_embeds_pdf_images(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "manual.pdf").write_bytes(b"%PDF-1.4")
    (docs / "index.md").write_text(
        "# PDF\n\n"
        "![Manual](manual.pdf#page=2)"
        '{ type="application/pdf" style="width:100%" }\n',
        encoding="utf-8",
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        "site_name: PDF\nplugins:\n  - mkdocs-pdf\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    page = BeautifulSoup(html, "html.parser")
    embed = page.select_one("embed")
    assert embed is not None
    assert embed["src"] == "manual.pdf#page=2"
    assert embed["type"] == "application/pdf"
    assert embed["style"] == "width:100%"
    assert embed["alt"] == "Manual"
    assert page.select_one('img[type="application/pdf"]') is None
    assert (tmp_path / "site" / "manual.pdf").read_bytes() == b"%PDF-1.4"
