# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

"""Integration tests for MkDocs-compatible search artifacts."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def _write_project(root: Path, *, plugins: str) -> Path:
    """Create a representative search project."""
    docs = root / "docs"
    (docs / "guide").mkdir(parents=True)
    (docs / "index.md").write_text(
        """\
---
tags:
  - alpha
  - beta
---

# Landing

Intro with <small>fine print</small>.

## Overview

Overview body.
""",
        encoding="utf-8",
    )
    (docs / "guide" / "topic.md").write_text(
        """\
---
title: Metadata title
tags:
  - guide
---

Preface before a heading.

## Details

Detailed body.
""",
        encoding="utf-8",
    )
    config = root / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Search
nav:
  - Home: index.md
  - Guides:
      - Topic: guide/topic.md
plugins:
{plugins}
""",
        encoding="utf-8",
    )
    return config


def _read_index(root: Path) -> dict[str, Any]:
    """Read the generated search index."""
    return json.loads((root / "site" / "search.json").read_text())


def test_search_artifacts_match_mkdocs_contract(tmp_path: Path) -> None:
    """Search output preserves ordering, page facts, and offline framing."""
    config = _write_project(
        tmp_path,
        plugins='  - search:\n      separator: "[\\\\s-]+"\n  - offline',
    )
    zensical.build(str(config), _BUILD_OPTIONS)

    expected = {
        "config": {"lang": ["en"], "separator": "[\\s-]+"},
        "items": [
            {
                "location": "index.html",
                "level": 1,
                "title": "Landing",
                "text": "<p>Intro with <small>fine print</small>.</p>",
                "path": ["Landing"],
                "tags": ["alpha", "beta"],
            },
            {
                "location": "index.html#overview",
                "level": 2,
                "title": "Overview",
                "text": "<p>Overview body.</p>",
                "path": ["Landing"],
                "tags": ["alpha", "beta"],
            },
            {
                "location": "guide/topic.html",
                "level": 1,
                "title": "Metadata title",
                "text": "<p>Preface before a heading.</p>",
                "path": ["Guides", "Metadata title"],
                "tags": ["guide"],
            },
            {
                "location": "guide/topic.html#details",
                "level": 2,
                "title": "Details",
                "text": "<p>Detailed body.</p>",
                "path": ["Guides", "Metadata title"],
                "tags": ["guide"],
            },
        ],
    }
    assert _read_index(tmp_path) == expected

    compact = json.dumps(expected, separators=(",", ":"), ensure_ascii=False)
    assert (tmp_path / "site" / "search.js").read_text() == (
        f"var __index = {compact};"
    )


def test_search_exclusion_and_disabled_output(tmp_path: Path) -> None:
    """Excluded pages contribute no items and disabled search stays valid."""
    config = _write_project(tmp_path, plugins="  search:\n    enabled: true")
    topic = tmp_path / "docs" / "guide" / "topic.md"
    topic.write_text(
        """\
---
search:
  exclude: true
---

# Hidden

Not indexed.
""",
        encoding="utf-8",
    )
    zensical.build(str(config), _BUILD_OPTIONS)
    assert [item["title"] for item in _read_index(tmp_path)["items"]] == [
        "Landing",
        "Overview",
    ]

    all_excluded = tmp_path / "all-excluded"
    all_excluded.mkdir()
    config = _write_project(all_excluded, plugins="  - search")
    for page in (all_excluded / "docs").rglob("*.md"):
        page.write_text(
            "---\nsearch:\n  exclude: true\n---\n\n# Hidden\n",
            encoding="utf-8",
        )
    zensical.build(str(config), _BUILD_OPTIONS)
    assert _read_index(all_excluded)["items"] == []

    disabled = tmp_path / "disabled"
    disabled.mkdir()
    config = _write_project(disabled, plugins="  search:\n    enabled: false")
    zensical.build(str(config), _BUILD_OPTIONS)
    assert _read_index(disabled)["items"] == []


def test_search_exclusion_attribute_is_removed_from_page(
    tmp_path: Path,
) -> None:
    """Search pragmas affect the index but do not leak into final HTML."""
    config = _write_project(tmp_path, plugins="  - search")
    (tmp_path / "docs" / "index.md").write_text(
        """\
# Landing

Visible body.

<div data-search-exclude><p>Hidden body.</p></div>
""",
        encoding="utf-8",
    )
    zensical.build(str(config), _BUILD_OPTIONS)

    index = _read_index(tmp_path)
    assert "Visible body." in index["items"][0]["text"]
    assert "Hidden body." not in index["items"][0]["text"]

    page = (tmp_path / "site" / "index.html").read_text()
    assert "Hidden body." in page
    assert "data-search-exclude" not in page


def test_search_rebuild_replaces_changed_and_removed_pages(
    tmp_path: Path,
) -> None:
    """Successive builds do not retain stale page search facts."""
    config = _write_project(tmp_path, plugins="  - search")
    zensical.build(str(config), _BUILD_OPTIONS)

    index = tmp_path / "docs" / "index.md"
    index.write_text("# Changed\n\nFresh body.\n", encoding="utf-8")
    (tmp_path / "docs" / "guide" / "topic.md").unlink()
    zensical.build(str(config), _BUILD_OPTIONS)

    assert _read_index(tmp_path)["items"] == [
        {
            "location": "",
            "level": 1,
            "title": "Changed",
            "text": "<p>Fresh body.</p>",
            "path": ["Changed"],
            "tags": [],
        }
    ]
