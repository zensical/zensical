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

"""Verify HTML escaping of page and site descriptions."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
import yaml
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("source", ["page", "site"])
@pytest.mark.parametrize(
    "description",
    [
        'This has "quotes" inside it',
        "Tom's guide & reference",
        '<script>alert("description")</script>',
        "Literal &quot; entity and café 日本語",
    ],
)
def test_descriptions_are_html_escaped(
    tmp_path: Path, source: str, description: str
) -> None:
    """Preserve the full description as a single HTML attribute."""
    # A site fallback is configured even when a page description is supplied.
    docs = tmp_path / "docs"
    docs.mkdir()
    metadata = {}
    if source == "page":
        metadata["description"] = description
    front_matter = yaml.safe_dump(metadata, allow_unicode=True)
    (docs / "index.md").write_text(
        f"---\n{front_matter}---\n# Description\n\n**Page content**\n",
        encoding="utf-8",
    )
    site_description = description if source == "site" else "Site fallback"
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        yaml.safe_dump({
            "site_name": "Descriptions",
            "site_description": site_description,
        }),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": True})

    # Attribute boundaries and literal entities must be preserved after parsing.
    output = (tmp_path / "site/index.html").read_text(encoding="utf-8")
    soup = BeautifulSoup(output, "html.parser")
    tags = soup.find_all("meta", attrs={"name": "description"})
    assert len(tags) == 1
    assert tags[0].attrs == {"name": "description", "content": description}
    assert "<strong>Page content</strong>" in output
