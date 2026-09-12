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

"""Integration coverage for autorefs configuration and native rendering."""

from __future__ import annotations

from html.parser import HTMLParser
from typing import TYPE_CHECKING, Any

import pytest
import yaml

import zensical

if TYPE_CHECKING:
    from pathlib import Path


class _Links(HTMLParser):
    def __init__(self, content: str) -> None:
        super().__init__(convert_charrefs=True)
        self.links: list[dict[str, str | None]] = []
        self.feed(content)

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        attributes = dict(attrs)
        if tag == "a" and "autorefs" in (attributes.get("class") or "").split():
            self.links.append(attributes)


def _write_project(
    root: Path,
    options: dict[str, Any] | None,
    *,
    features: tuple[str, ...] = (),
) -> Path:
    docs = root / "docs"
    guide = docs / "guide"
    overrides = root / "overrides"
    guide.mkdir(parents=True, exist_ok=True)
    overrides.mkdir(exist_ok=True)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "a.md").write_text("# First {#shared}\n", encoding="utf-8")
    (guide / "near.md").write_text("# Nearby {#shared}\n", encoding="utf-8")
    (guide / "index.md").write_text(
        "# Guide\n\n[Target][shared]\n\n"
        '<autoref identifier="shared" optional>API target</autoref>\n',
        encoding="utf-8",
    )
    (overrides / "main.html").write_text("{{ page.content }}", encoding="utf-8")
    extensions = ["attr_list"]
    if options is None:
        extensions.append("zensical.extensions.autorefs")
    config = root / "mkdocs.yml"
    config.write_text(
        yaml.safe_dump(
            {
                "site_name": "Autorefs settings",
                "theme": {
                    "custom_dir": str(overrides),
                    "features": list(features),
                },
                "plugins": {"autorefs": options} if options is not None else {},
                "markdown_extensions": extensions,
            }
        ),
        encoding="utf-8",
    )
    return config


def _build_links(
    root: Path, config: Path, *, clean: bool = True
) -> list[dict[str, str | None]]:
    zensical.build(str(config), {"clean": clean, "strict": False})
    content = (root / "site" / "guide" / "index.html").read_text(
        encoding="utf-8"
    )
    return _Links(content).links


def test_resolve_closest_changes_between_builds(tmp_path: Path) -> None:
    for index, (options, expected) in enumerate(
        [
            ({}, "../a/#shared"),
            ({"resolve_closest": True}, "near/#shared"),
            ({"resolve_closest": False}, "../a/#shared"),
        ]
    ):
        config = _write_project(tmp_path, options)
        links = _build_links(tmp_path, config, clean=index == 0)
        assert [link["href"] for link in links] == [expected, expected]


@pytest.mark.parametrize(
    ("mode", "features", "has_title"),
    [
        (True, (), True),
        (True, ("navigation.instant.preview",), True),
        (False, (), False),
        ("external", (), False),
        ("auto", (), True),
        ("auto", ("navigation.instant.preview",), False),
    ],
)
def test_link_titles(
    tmp_path: Path,
    mode: bool | str,
    features: tuple[str, ...],
    has_title: bool,
) -> None:
    config = _write_project(tmp_path, {"link_titles": mode}, features=features)
    links = _build_links(tmp_path, config)
    assert len(links) == 2
    assert all(("title" in link) == has_title for link in links)
    if has_title:
        assert links[0]["title"] == "First"


@pytest.mark.parametrize(
    ("mode", "features", "title"),
    [
        (True, ("content.tooltips",), "First (shared)"),
        (False, (), "First (<code>shared</code>)"),
        ("auto", (), "First (shared)"),
        ("auto", ("content.tooltips",), "First (<code>shared</code>)"),
    ],
)
def test_strip_title_tags(
    tmp_path: Path,
    mode: bool | str,
    features: tuple[str, ...],
    title: str,
) -> None:
    config = _write_project(
        tmp_path, {"strip_title_tags": mode}, features=features
    )
    links = _build_links(tmp_path, config)
    assert links[1]["title"] == title


@pytest.mark.parametrize("preview", [False, True])
def test_implicit_autorefs_uses_automatic_title_defaults(
    tmp_path: Path, preview: bool
) -> None:
    features = ("navigation.instant.preview",) if preview else ()
    config = _write_project(tmp_path, None, features=features)
    links = _build_links(tmp_path, config)
    assert len(links) == 2
    assert all(("title" in link) != preview for link in links)


@pytest.mark.parametrize("record_backlinks", [False, True])
@pytest.mark.parametrize(
    ("options", "href", "titles"),
    [
        (
            {
                "resolve_closest": True,
                "link_titles": True,
                "strip_title_tags": False,
            },
            "near/#shared",
            [
                "Nearby",
                "Nearby (<code>shared</code>)",
                "Nearby (<code>shared</code>)",
            ],
        ),
        (
            {"resolve_closest": False, "link_titles": False},
            "../a/#shared",
            [None, None, None],
        ),
        (
            {
                "resolve_closest": False,
                "link_titles": True,
                "strip_title_tags": True,
            },
            "../a/#shared",
            ["First", "First (shared)", "First (shared)"],
        ),
    ],
)
def test_settings_apply_to_cached_and_template_references(
    tmp_path: Path,
    record_backlinks: bool,
    options: dict[str, Any],
    href: str,
    titles: list[str | None],
) -> None:
    # Exercise both registry paths with references in Markdown and a template.
    config = _write_project(tmp_path, options)
    project = yaml.safe_load(config.read_text(encoding="utf-8"))
    project["markdown_extensions"].append(
        {"zensical.extensions.autorefs": {"record_backlinks": record_backlinks}}
    )
    config.write_text(yaml.safe_dump(project), encoding="utf-8")
    reference = '<autoref identifier="shared" optional>Template</autoref>'
    (tmp_path / "overrides" / "main.html").write_text(
        "{{ page.content }}" + reference,
        encoding="utf-8",
    )

    # The same settings must apply on the initial build and when reusing caches.
    for clean in [True, False]:
        links = _build_links(tmp_path, config, clean=clean)

        assert [link["href"] for link in links] == [href, href, href]
        assert [link.get("title") for link in links] == titles
