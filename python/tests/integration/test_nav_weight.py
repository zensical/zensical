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

"""Integration tests for native mkdocs-nav-weight 0.3.0 compatibility."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import pytest
import yaml
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path

    from bs4 import Tag


_TEMPLATE = """\
{% macro render(items, depth) %}
{% for item in items %}
<item depth="{{ depth }}" title="{{ item.title or '' }}" url="{{ item.url or '' }}" />
{{ render(item.children, depth + 1) }}
{% endfor %}
{% endmacro %}
{{ render(nav.items, 0) }}
{% for item in nav.pages %}<flat url="{{ item.url }}" />{% endfor %}
{% for item in page.ancestors %}<ancestor title="{{ item.title }}" />{% endfor %}
<current title="{{ page.title }}"
         previous="{{ page.previous_page.url if page.previous_page else '' }}"
         next="{{ page.next_page.url if page.next_page else '' }}" />
"""  # noqa: E501


def _markdown(title: str, **metadata: Any) -> str:
    """Create a Markdown source with the metadata relevant to a test."""
    front_matter = yaml.safe_dump(metadata) if metadata else ""
    return f"---\n{front_matter}---\n# {title}\n"


def _project(
    root: Path,
    documents: dict[str, str],
    settings: dict[str, Any] | None = None,
    nav: list[Any] | None = None,
) -> Path:
    """Write a project with observable navigation and page connections."""
    overrides = root / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(_TEMPLATE, encoding="utf-8")

    for name, content in documents.items():
        path = root / "docs" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    config: dict[str, Any] = {
        "site_name": "Navigation weights",
        "theme": {"name": "material", "custom_dir": "overrides"},
        "plugins": [{"mkdocs-nav-weight": settings or {}}],
    }
    if nav is not None:
        config["nav"] = nav
    path = root / "mkdocs.yml"
    path.write_text(yaml.safe_dump(config, sort_keys=False), encoding="utf-8")
    return path


def _build(config: Path, *, strict: bool = False) -> None:
    zensical.build(str(config), {"clean": False, "strict": strict})


def _output(root: Path, page: str = "index.html") -> BeautifulSoup:
    return BeautifulSoup(
        (root / "site" / page).read_text(encoding="utf-8"), "html.parser"
    )


def _items(output: BeautifulSoup) -> list[tuple[int, str, str]]:
    return [
        (int(str(item["depth"])), str(item["title"]), str(item["url"]))
        for item in output.find_all("item")
    ]


def _pages(output: BeautifulSoup) -> list[str]:
    return [str(item["url"]) for item in output.find_all("flat")]


def _current(root: Path, page: str) -> Tag:
    """Return the current page element and assert that it exists."""
    current = _output(root, page).select_one("current")
    assert current is not None
    return current


def test_sorts_pages_sections_and_empty_indexes(tmp_path: Path) -> None:
    # Section metadata is supplied by an empty index; all sources are rendered.
    config = _project(
        tmp_path,
        {
            "index.md": "# Home\n",
            "a.md": _markdown("Later", weight=3.5),
            "z.md": _markdown("Early", weight=-2),
            "b.md": "# Default\n",
            "guide/index.md": _markdown(
                "Handbook", weight=-1, retitled=True, empty=True
            ),
            "guide/a.md": _markdown("Second", weight=2),
            "guide/z.md": _markdown("First", weight=1),
            "reference/README.md": _markdown("Reference index", weight=5),
            "reference/topic.md": "# Topic\n",
            "unindexed/topic.md": "# Unindexed\n",
        },
        {"default_page_weight": 4},
    )

    _build(config)
    output = _output(tmp_path)

    assert _items(output) == [
        (0, "Home", ""),
        (0, "Early", "z/"),
        (0, "Handbook", ""),
        (1, "First", "guide/z/"),
        (1, "Second", "guide/a/"),
        (0, "Unindexed", ""),
        (1, "Unindexed", "unindexed/topic/"),
        (0, "Later", "a/"),
        (0, "Default", "b/"),
        (0, "Reference", ""),
        (1, "Reference index", "reference/"),
        (1, "Topic", "reference/topic/"),
    ]
    assert "guide/" not in _pages(output)
    assert (tmp_path / "site" / "guide" / "index.html").is_file()
    assert _current(tmp_path, "guide/z/index.html")["next"] == "guide/a/"


@pytest.mark.parametrize("included", [False, True])
def test_hides_pages_and_isolates_hidden_sections(
    tmp_path: Path,
    included: bool,
) -> None:
    config = _project(
        tmp_path,
        {
            # Root index metadata is ignored, as in the released plugin.
            "index.md": _markdown(
                "Home", headless=True, empty=True, retitled=True, weight=100
            ),
            "a.md": "# Visible\n",
            "hidden.md": _markdown("Hidden page", headless=True, weight=5),
            "secret/index.md": _markdown("Secret", headless=True),
            "secret/next.md": "# Secret next\n",
            "secret/hidden.md": _markdown("Hidden child", headless=True),
        },
        {"headless_included": included},
    )

    _build(config)
    output = _output(tmp_path)

    assert _items(output) == [(0, "Home", ""), (0, "Visible", "a/")]
    expected_pages = ["", "a/"]
    if included:
        expected_pages += [
            "hidden/",
            "secret/",
            "secret/next/",
            "secret/hidden/",
        ]
    assert _pages(output) == expected_pages

    # Page sequences within hidden sections are kept separate.
    secret = _current(tmp_path, "secret/index.html")
    assert secret["previous"] == ""
    assert secret["next"] == "secret/next/"
    assert _current(tmp_path, "secret/next/index.html")["previous"] == "secret/"
    hidden = _current(tmp_path, "hidden/index.html")
    assert hidden["previous"] == hidden["next"] == ""
    hidden_child = _output(tmp_path, "secret/hidden/index.html")
    assert [item["title"] for item in hidden_child.find_all("ancestor")] == [
        "Secret"
    ]
    assert _current(tmp_path, "a/index.html")["next"] == ""


@pytest.mark.parametrize("reverse", [False, True])
def test_preserves_equal_weight_order_in_configured_navigation(
    tmp_path: Path,
    reverse: bool,
) -> None:
    config = _project(
        tmp_path,
        {
            "index.md": "# Home\n",
            "first.md": _markdown("First", weight=1),
            "second.md": _markdown("Second", weight=True),
            "group/README.md": _markdown("Source title", weight=2),
            "group/child.md": _markdown("Child", weight=-4),
        },
        {"reverse": reverse, "index_weight": -3, "section_renamed": True},
        [
            {"Home": "index.md"},
            {"Second alias": "second.md"},
            {"First alias": "first.md"},
            {"Website": "https://example.com"},
            {"Group": [{"Index alias": "group/README.md"}, "group/child.md"]},
        ],
    )

    _build(config)
    output = _output(tmp_path)

    expected = [
        (0, "Home", ""),
        (0, "Website", "https://example.com"),
        (0, "Second alias", "second/"),
        (0, "First alias", "first/"),
        (0, "Index alias", ""),
        (1, "Child", "group/child/"),
        (1, "Index alias", "group/"),
    ]
    if reverse:
        expected = [
            expected[4],
            expected[6],
            expected[5],
            expected[2],
            expected[3],
            expected[1],
            expected[0],
        ]
    assert _items(output) == expected
    first = _current(tmp_path, "first/index.html")
    assert first["title"] == "First alias"
    second = _current(tmp_path, "second/index.html")
    assert second["next"] == "first/"
    if reverse:
        assert first["next"] == ""
    else:
        assert second["previous"] == ""


@pytest.mark.parametrize("warning", [False, True])
def test_invalid_metadata_falls_back_and_obeys_strict_mode(
    tmp_path: Path,
    warning: bool,
) -> None:
    config = _project(
        tmp_path,
        {
            "index.md": "# Home\n",
            "first.md": _markdown("First", weight="1", headless="true"),
            "second.md": _markdown("Second", weight=2),
            "group/index.md": _markdown(
                "Group index", weight=[], retitled=1, empty="true"
            ),
        },
        {"warning": warning, "default_page_weight": 3},
    )

    if warning:
        with pytest.raises(
            RuntimeError, match="mkdocs-nav-weight reported warnings"
        ):
            _build(config, strict=True)
    else:
        _build(config, strict=True)

        assert _items(_output(tmp_path)) == [
            (0, "Home", ""),
            (0, "Second", "second/"),
            (0, "First", "first/"),
            (0, "Group", ""),
            (1, "Group index", "group/"),
        ]


def test_disabled_plugin_preserves_navigation(tmp_path: Path) -> None:
    config = _project(
        tmp_path,
        {
            "index.md": "# Home\n",
            "a.md": _markdown("Alpha", headless=True, weight=10),
            "z.md": _markdown("Zeta", weight=-10),
        },
        {"enabled": False},
    )

    _build(config)

    assert _items(_output(tmp_path)) == [
        (0, "Home", ""),
        (0, "Alpha", "a/"),
        (0, "Zeta", "z/"),
    ]


def test_metadata_changes_update_cached_navigation(tmp_path: Path) -> None:
    config = _project(
        tmp_path,
        {
            "index.md": "# Home\n",
            "a.md": _markdown("Alpha", weight=1),
            "z.md": _markdown("Zeta", weight=2),
        },
    )
    _build(config)
    assert _pages(_output(tmp_path)) == ["", "a/", "z/"]

    # Ordering, titles, and visibility must be updated during a cached rebuild.
    (tmp_path / "docs" / "z.md").write_text(
        _markdown("Renamed", weight=-1), encoding="utf-8"
    )
    (tmp_path / "docs" / "a.md").write_text(
        _markdown("Alpha", headless=True), encoding="utf-8"
    )
    _build(config)

    assert _items(_output(tmp_path)) == [(0, "Home", ""), (0, "Renamed", "z/")]


@pytest.mark.parametrize("plugin", ["awesome-nav", "literate-nav"])
def test_applies_weights_after_other_navigation_plugins(
    tmp_path: Path, plugin: str
) -> None:
    config = _project(
        tmp_path,
        {
            "index.md": "# Home\n",
            "a.md": _markdown("Alpha", weight=2),
            "z.md": _markdown("Zeta", weight=1),
        },
    )
    data = yaml.safe_load(config.read_text(encoding="utf-8"))
    data["plugins"].append(plugin)
    config.write_text(yaml.safe_dump(data), encoding="utf-8")

    if plugin == "awesome-nav":
        control = tmp_path / "docs" / ".nav.yml"
        control.write_text(
            "nav:\n  - index.md\n  - First alias: a.md\n"
            "  - Second alias: z.md\n",
            encoding="utf-8",
        )
    else:
        control = tmp_path / "docs" / "SUMMARY.md"
        control.write_text(
            "* [Home](index.md)\n* [First alias](a.md)\n"
            "* [Second alias](z.md)\n",
            encoding="utf-8",
        )

    _build(config)

    assert _items(_output(tmp_path)) == [
        (0, "Home", ""),
        (0, "Second alias", "z/"),
        (0, "First alias", "a/"),
    ]


def test_preserves_large_integer_weights(tmp_path: Path) -> None:
    # Adjacent integers must remain distinct above floating-point precision.
    config = _project(
        tmp_path,
        {
            "index.md": "# Home\n",
            "a.md": _markdown("Larger", weight=9007199254740993),
            "b.md": _markdown("Float", weight=9007199254740992.0),
            "z.md": _markdown("Smaller", weight=9007199254740992),
        },
    )

    _build(config)

    assert _pages(_output(tmp_path)) == ["", "b/", "z/", "a/"]


@pytest.mark.parametrize("enabled", [None, False, True])
def test_connections_skip_links_only_when_nav_weight_is_enabled(
    tmp_path: Path,
    enabled: bool | None,
) -> None:
    # External URLs and unresolved references are kept in configured navigation.
    config = _project(
        tmp_path,
        {"index.md": "# Home\n", "last.md": "# Last\n"},
        {"enabled": enabled} if enabled is not None else None,
        [
            {"Home": "index.md"},
            {"Website": "https://example.com"},
            {"Missing": "missing.md"},
            {"Download": "manual.pdf"},
            {"Notebook": "notebook.html"},
            {"Last": "last.md"},
            {"Author's website": "https://author.example.com"},
        ],
    )
    if enabled is None:
        settings = yaml.safe_load(config.read_text(encoding="utf-8"))
        settings["plugins"] = []
        config.write_text(yaml.safe_dump(settings), encoding="utf-8")

    _build(config)
    home = _current(tmp_path, "index.html")
    last = _current(tmp_path, "last/index.html")

    if enabled:
        assert home["next"] == "last/"
        assert last["previous"] == ""
        assert last["next"] == ""
    else:
        assert home["next"] == "https://example.com"
        assert last["previous"] == "notebook.html"
        assert last["next"] == "https://author.example.com"
