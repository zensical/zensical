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

"""Integration tests for native mkdocs-literate-nav compatibility."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import pytest
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def _write_template(root: Path) -> None:
    """Write a compact recursive navigation oracle."""
    overrides = root / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(
        """\
{% macro render(items, depth) %}
{% for item in items %}
<item depth="{{ depth }}" title="{{ item.title or '' }}"
      url="{{ item.url or '' }}" />
{{ render(item.children, depth + 1) }}
{% endfor %}
{% endmacro %}
{{ render(nav.items, 0) }}
""",
        encoding="utf-8",
    )


def _items(root: Path) -> list[tuple[int, str, str]]:
    """Extract the template's normalized navigation records."""
    output_path = root / "site" / "index.html"
    if not output_path.exists():
        output_path = next((root / "site").rglob("*.html"))
    output = output_path.read_text()
    soup = BeautifulSoup(output, "html.parser")
    return [
        (
            int(str(item["depth"])),
            str(item["title"]),
            str(item["url"]),
        )
        for item in soup.find_all("item")
    ]


def test_resolves_markers_nested_files_wildcards_and_external_links(
    tmp_path: Path,
) -> None:
    """The complete native pipeline reproduces a mixed literate nav."""
    docs = tmp_path / "docs"
    api = docs / "guide" / "api"
    api.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "ignored.md").write_text("# Ignored\n", encoding="utf-8")
    (docs / "SUMMARY.md").write_text(
        """\
* [Ignored before marker](ignored.md)

<!--nav-->

* [Home](index.md)
* [Guide](guide/)
* [Project](https://example.com/project)
""",
        encoding="utf-8",
    )
    (docs / "guide" / "SUMMARY.md").write_text(
        """\
* [Overview](index.md)
* [Start](start.md)
* API
    * api/*.md
* *
""",
        encoding="utf-8",
    )
    (docs / "guide" / "index.md").write_text("# Overview\n", encoding="utf-8")
    (docs / "guide" / "start.md").write_text("# Start\n", encoding="utf-8")
    (docs / "guide" / "advanced.md").write_text(
        "# Advanced\n", encoding="utf-8"
    )
    (api / "one.md").write_text("# One\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - literate-nav
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Home", ""),
        (0, "Guide", ""),
        (1, "Overview", "guide/"),
        (1, "Start", "guide/start/"),
        (1, "API", ""),
        (2, "One", "guide/api/one/"),
        (1, "Advanced", "guide/advanced/"),
        (0, "Project", "https://example.com/project"),
    ]


def test_resolves_configured_directory_through_nested_literate_nav(
    tmp_path: Path,
) -> None:
    """A titled directory in configured nav delegates to its own file."""
    docs = tmp_path / "docs"
    guide = docs / "guide"
    guide.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (guide / "SUMMARY.md").write_text("* [Start](start.md)\n", encoding="utf-8")
    (guide / "start.md").write_text("# Start\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - literate-nav
nav:
  - Home: index.md
  - Guide: guide/
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Home", ""),
        (0, "Guide", ""),
        (1, "Start", "guide/start/"),
    ]


def test_preserves_entity_spellings_in_titles(tmp_path: Path) -> None:
    """The HTML transport does not collapse distinct Markdown title text."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "SUMMARY.md").write_text(
        """\
* [a&amp;b](a.md)
* [a&b](b.md)
* [a&amp;amp;b](c.md)
* [\\__init__](d.md)
* [\\`hi`](e.md)
""",
        encoding="utf-8",
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - literate-nav
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = next((tmp_path / "site").rglob("*.html")).read_text()
    assert 'title="a&amp;b"' in output
    assert 'title="a&b"' in output
    assert 'title="a&amp;amp;b"' in output
    assert 'title="__init__"' in output
    assert 'title="`hi`"' in output


def test_marker_preserves_reference_definitions_from_the_complete_document(
    tmp_path: Path,
) -> None:
    """The marker changes list selection without isolating Markdown state."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "SUMMARY.md").write_text(
        """\
[guide]: guide.md

- [Ignored](ignored.md)

<!--nav-->
- [Earlier](ignored.md)

<!--nav-->
- [Guide][guide]

Gap

- [Later](ignored.md)
""",
        encoding="utf-8",
    )
    (docs / "guide.md").write_text("# Guide\n", encoding="utf-8")
    (docs / "ignored.md").write_text("# Ignored\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - literate-nav
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert _items(tmp_path) == [(0, "Guide", "guide/")]


def test_applies_plugin_local_tab_length(tmp_path: Path) -> None:
    """Plugin-local indentation controls the navigation Markdown parser."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "SUMMARY.md").write_text(
        "- Guide\n  - [Start](start.md)\n", encoding="utf-8"
    )
    (docs / "start.md").write_text("# Start\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - literate-nav:
      tab_length: 2
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Guide", ""),
        (1, "Start", "start/"),
    ]


def test_directory_wildcards_do_not_consume_files(tmp_path: Path) -> None:
    """A slash wildcard leaves files available to following wildcards."""
    docs = tmp_path / "docs"
    section = docs / "section2"
    section.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "SUMMARY.md").write_text("- */\n- *.md\n", encoding="utf-8")
    (docs / "item1.md").write_text("# Item 1\n", encoding="utf-8")
    (docs / "item2.md").write_text("# Item 2\n", encoding="utf-8")
    (section / "item.md").write_text("# Section item\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - literate-nav
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Section2", ""),
        (1, "Section item", "section2/item/"),
        (0, "Item 1", "item1/"),
        (0, "Item 2", "item2/"),
    ]


def test_explicitly_empty_literate_navigation_stays_empty(
    tmp_path: Path,
) -> None:
    """An exhausted wildcard must not reactivate automatic navigation."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "SUMMARY.md").write_text("- *\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - literate-nav
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert _items(tmp_path) == []


@pytest.mark.parametrize(
    "summary",
    [
        "* Empty section\n",
        "* **[Obscured](page.md)**\n",
        "* [First](first.md)[Second](second.md)\n",
        "* [Page](page.md) trailing text\n",
        "1. * [Item](section/item.md)\n",
        "1. Section *one*\n    * [Item](section/item.md)\n",
    ],
)
def test_rejects_ambiguous_navigation_items(
    tmp_path: Path, summary: str
) -> None:
    """Invalid list items fail instead of producing surprising navigation."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "SUMMARY.md").write_text(summary, encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Literate navigation
plugins:
  - literate-nav
""",
        encoding="utf-8",
    )

    with pytest.raises(RuntimeError):
        zensical.build(str(config), _BUILD_OPTIONS)
