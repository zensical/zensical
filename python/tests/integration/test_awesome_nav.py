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

"""Integration tests for native mkdocs-awesome-nav compatibility."""

from __future__ import annotations

import subprocess
import sys
import time
from typing import TYPE_CHECKING, Any

import pytest
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from collections.abc import Callable
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
      url="{{ item.url or '' }}" index="{{ item.is_index }}" />
{{ render(item.children, depth + 1) }}
{% endfor %}
{% endmacro %}
{{ render(nav.items, 0) }}
""",
        encoding="utf-8",
    )


def _items(root: Path) -> list[tuple[int, str, str]]:
    output_path = root / "site" / "index.html"
    if not output_path.exists():
        output_path = next((root / "site").rglob("*.html"))
    output = output_path.read_text()
    soup = BeautifulSoup(output, "html.parser")
    return [
        (int(str(item["depth"])), str(item["title"]), str(item["url"]))
        for item in soup.find_all("item")
    ]


def _items_or_none(root: Path) -> list[tuple[int, str, str]] | None:
    try:
        return _items(root)
    except (FileNotFoundError, StopIteration):
        return None


def _index_flags(root: Path) -> list[bool]:
    """Extract whether each normalized navigation item is an index page."""
    output_path = root / "site" / "index.html"
    if not output_path.exists():
        output_path = next((root / "site").rglob("*.html"))
    soup = BeautifulSoup(output_path.read_text(), "html.parser")
    return [
        str(item["index"]).lower() == "true" for item in soup.find_all("item")
    ]


def _write_config(root: Path, plugin: str = "awesome-nav") -> Path:
    config = root / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Awesome navigation
theme:
  name: material
  custom_dir: overrides
plugins:
  - {plugin}
""",
        encoding="utf-8",
    )
    return config


def test_resolves_nested_configuration_patterns_options_and_links(
    tmp_path: Path,
) -> None:
    """The native pipeline resolves a representative awesome-nav project."""
    docs = tmp_path / "docs"
    api = docs / "guide" / "api"
    api.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "z10.md").write_text("# Ten\n", encoding="utf-8")
    (docs / "z2.md").write_text("# Two\n", encoding="utf-8")
    (docs / ".nav.yml").write_text(
        """\
nav:
  - index.md
  - Guide: guide
  - Resources:
      - z*.md
  - Website: https://example.com
""",
        encoding="utf-8",
    )
    guide = docs / "guide"
    (guide / "index.md").write_text(
        "---\ntitle: Guide landing\n---\n# Overview\n", encoding="utf-8"
    )
    (guide / "start.md").write_text("# Start\n", encoding="utf-8")
    (guide / "draft.hidden.md").write_text("# Hidden\n", encoding="utf-8")
    (guide / ".nav.yml").write_text(
        """\
use_index_title: true
ignore: "*.hidden.md"
sort:
  by: filename
  type: natural
nav:
  - index.md
  - glob: "*.md"
  - api
""",
        encoding="utf-8",
    )
    (api / "one.md").write_text("# One\n", encoding="utf-8")

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Home", ""),
        (0, "Guide", ""),
        (1, "Guide landing", "guide/"),
        (1, "Start", "guide/start/"),
        (1, "Api", ""),
        (2, "One", "guide/api/one/"),
        (0, "Resources", ""),
        (1, "Two", "z2/"),
        (1, "Ten", "z10/"),
        (0, "Website", "https://example.com"),
    ]


def test_explicit_pages_are_claimed_before_earlier_patterns(
    tmp_path: Path,
) -> None:
    """Resolution priority is independent of declaration position."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    for name in ("index.md", "other.md", "last.md"):
        (docs / name).write_text(f"# {name}\n", encoding="utf-8")
    (docs / ".nav.yml").write_text(
        'nav:\n  - "*"\n  - Last: last.md\n', encoding="utf-8"
    )

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "index.md", ""),
        (0, "other.md", "other/"),
        (0, "Last", "last/"),
    ]


def test_default_navigation_discovers_nested_directories_without_config(
    tmp_path: Path,
) -> None:
    """The default index-first navigation also works without `.nav.yml`."""
    docs = tmp_path / "docs"
    guide = docs / "guide"
    guide.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "other.md").write_text("# Other\n", encoding="utf-8")
    (guide / "start.md").write_text("# Start\n", encoding="utf-8")

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Home", ""),
        (0, "Other", "other/"),
        (0, "Guide", ""),
        (1, "Start", "guide/start/"),
    ]


def test_default_navigation_prefers_index_over_readme(tmp_path: Path) -> None:
    """MkDocs suppresses a README when the same directory has an index."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Index\n", encoding="utf-8")
    (docs / "README.md").write_text("# Readme\n", encoding="utf-8")
    (docs / "other.md").write_text("# Other\n", encoding="utf-8")

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Index", ""),
        (0, "Other", "other/"),
    ]


def test_nested_index_is_classified_for_theme_section_merging(
    tmp_path: Path,
) -> None:
    """Nested index paths retain the index marker used by Material's theme."""
    docs = tmp_path / "docs"
    section = docs / "tech-stack"
    section.mkdir(parents=True)
    _write_template(tmp_path)
    (section / "index.md").write_text(
        "# Tech-Stack Home Page Title\n", encoding="utf-8"
    )
    (section / "page.md").write_text("# Page\n", encoding="utf-8")
    (section / ".nav.yaml").write_text(
        "title: Tech-Stack\nnav: ['*']\n", encoding="utf-8"
    )

    zensical.build(
        str(_write_config(tmp_path, "awesome-nav:\n      filename: .nav.yaml")),
        _BUILD_OPTIONS,
    )

    assert _items(tmp_path) == [
        (0, "Tech-Stack", ""),
        (1, "Tech-Stack Home Page Title", "tech-stack/"),
        (1, "Page", "tech-stack/page/"),
    ]
    assert _index_flags(tmp_path) == [False, True, False]


def test_explicit_page_title_precedes_metadata_and_heading(
    tmp_path: Path,
) -> None:
    """Awesome Nav titles become MkDocs-compatible page titles."""
    docs = tmp_path / "docs"
    docs.mkdir()
    overrides = tmp_path / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(
        "{{ page.title }}", encoding="utf-8"
    )
    (docs / "index.md").write_text(
        "---\ntitle: Metadata title\n---\n\n# Heading title\n",
        encoding="utf-8",
    )
    (docs / ".nav.yml").write_text(
        "nav:\n  - Configured title: index.md\n", encoding="utf-8"
    )

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert (tmp_path / "site" / "index.html").read_text() == "Configured title"


def test_pattern_options_hide_directories_flatten_and_sort_by_metadata(
    tmp_path: Path,
) -> None:
    """Pattern-local behavior is applied before matches are sorted."""
    docs = tmp_path / "docs"
    visible = docs / "visible"
    hidden = docs / "hidden"
    visible.mkdir(parents=True)
    hidden.mkdir()
    _write_template(tmp_path)
    (visible / "a.md").write_text("# Zed\n", encoding="utf-8")
    (visible / "b.md").write_text(
        "---\ntitle: 0 First\n---\n# Bee\n", encoding="utf-8"
    )
    (hidden / "page.md").write_text("# Hidden\n", encoding="utf-8")
    (hidden / ".nav.yml").write_text("hide: true\n", encoding="utf-8")
    (docs / ".nav.yml").write_text(
        """\
nav:
  - glob: "*/"
    flatten_single_child_sections: true
    sort:
      by: title
""",
        encoding="utf-8",
    )

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Visible", ""),
        (1, "0 First", "visible/b/"),
        (1, "Zed", "visible/a/"),
    ]


def test_inherits_ignore_and_append_unmatched_with_explicit_false_override(
    tmp_path: Path,
) -> None:
    """Child booleans override parents and ignore lists expand `$inherit`."""
    docs = tmp_path / "docs"
    guide = docs / "guide"
    guide.mkdir(parents=True)
    _write_template(tmp_path)
    (guide / "keep.md").write_text("# Keep\n", encoding="utf-8")
    (guide / "extra.md").write_text("# Extra\n", encoding="utf-8")
    (guide / "skip.hidden.md").write_text("# Hidden\n", encoding="utf-8")
    (guide / "skip.draft.md").write_text("# Draft\n", encoding="utf-8")
    (docs / ".nav.yml").write_text(
        """\
flatten_single_child_sections: true
append_unmatched: true
ignore: "*.hidden.md"
nav: [guide]
""",
        encoding="utf-8",
    )
    (guide / ".nav.yml").write_text(
        """\
flatten_single_child_sections: false
ignore:
  - $inherit
  - "*.draft.md"
nav: [keep.md]
""",
        encoding="utf-8",
    )

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Guide", ""),
        (1, "Keep", "guide/keep/"),
        (1, "Extra", "guide/extra/"),
    ]


def test_preserved_directory_name_precedes_index_title(tmp_path: Path) -> None:
    """Literal directory names win when both title options are enabled."""
    docs = tmp_path / "docs"
    section = docs / "literal-name"
    section.mkdir(parents=True)
    _write_template(tmp_path)
    (section / "index.md").write_text(
        "---\ntitle: Metadata title\n---\n# Index\n", encoding="utf-8"
    )
    (section / "other.md").write_text("# Other\n", encoding="utf-8")
    (docs / ".nav.yml").write_text(
        "preserve_directory_names: true\nuse_index_title: true\n",
        encoding="utf-8",
    )

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path)[0] == (0, "literal-name", "")


def test_flattening_keeps_directory_around_a_single_external_link(
    tmp_path: Path,
) -> None:
    """Upstream only flattens a lone page or section, never a link."""
    docs = tmp_path / "docs"
    section = docs / "links"
    section.mkdir(parents=True)
    _write_template(tmp_path)
    (section / "placeholder.md").write_text("# Placeholder\n", encoding="utf-8")
    (docs / ".nav.yml").write_text(
        "flatten_single_child_sections: true\nnav: [links]\n",
        encoding="utf-8",
    )
    (section / ".nav.yml").write_text(
        "nav:\n  - Website: https://example.com\n", encoding="utf-8"
    )

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Links", ""),
        (1, "Website", "https://example.com"),
    ]


def test_custom_filename_and_explicit_empty_navigation(tmp_path: Path) -> None:
    """The plugin option selects control files and preserves an empty nav."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "awesome.yml").write_text("nav: []\n", encoding="utf-8")

    plugin = "awesome-nav:\n      filename: awesome.yml"
    zensical.build(str(_write_config(tmp_path, plugin)), _BUILD_OPTIONS)

    assert _items(tmp_path) == []


def test_natural_sort_matches_upstream_numeric_and_grouped_case_order(
    tmp_path: Path,
) -> None:
    """Natural sorting treats extensions, integer runs and case like natsort."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    pages = {
        "2.md": "2",
        "2-suffix.md": "2 suffix",
        "2.5.md": "2.5",
        "10.md": "10",
        "numeric-a.md": "9",
        "numeric-z.md": "8",
        "A-upper.md": "A",
        "a-lower.md": "a",
        "B-upper.md": "B",
        "b-lower.md": "b",
    }
    for name, title in pages.items():
        (docs / name).write_text(
            f'---\ntitle: "{title}"\n---\n# Page\n', encoding="utf-8"
        )
    (docs / ".nav.yml").write_text("sort:\n  by: title\n", encoding="utf-8")

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert [title for _, title, _ in _items(tmp_path)] == [
        "2",
        "2 suffix",
        "2.5",
        "8",
        "9",
        "10",
        "A",
        "a",
        "B",
        "b",
    ]


def test_deep_explicit_directory_resolves_before_its_parent(
    tmp_path: Path,
) -> None:
    """A separately listed child directory is not consumed by its parent."""
    docs = tmp_path / "docs"
    nested = docs / "foo" / "bar"
    nested.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "foo" / "foo.md").write_text("# Foo\n", encoding="utf-8")
    (nested / "bar.md").write_text("# Bar\n", encoding="utf-8")
    (docs / ".nav.yml").write_text("nav: [foo, foo/bar]\n", encoding="utf-8")

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Foo", ""),
        (1, "Foo", "foo/foo/"),
        (0, "Bar", ""),
        (1, "Bar", "foo/bar/bar/"),
    ]


def test_recursive_directory_pattern_resolves_deepest_matches_first(
    tmp_path: Path,
) -> None:
    """A parent pattern match cannot consume a separately matched child."""
    docs = tmp_path / "docs"
    nested = docs / "foo" / "bar"
    nested.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "foo" / "foo.md").write_text("# Foo\n", encoding="utf-8")
    (nested / "bar.md").write_text("# Bar\n", encoding="utf-8")
    (docs / ".nav.yml").write_text("nav: ['**/']\n", encoding="utf-8")

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Foo", ""),
        (1, "Foo", "foo/foo/"),
        (0, "Bar", ""),
        (1, "Bar", "foo/bar/bar/"),
    ]


def test_globstar_flattens_pages_at_every_depth(tmp_path: Path) -> None:
    """A bare globstar claims pages directly and leaves directories empty."""
    docs = tmp_path / "docs"
    deep = docs / "bar" / "nested"
    deep.mkdir(parents=True)
    _write_template(tmp_path)
    (docs / "foo.md").write_text("# Root\n", encoding="utf-8")
    (docs / "bar" / "foo.md").write_text("# Child\n", encoding="utf-8")
    (deep / "foo.md").write_text("# Deep\n", encoding="utf-8")
    (docs / ".nav.yml").write_text("nav: ['**']\n", encoding="utf-8")

    zensical.build(str(_write_config(tmp_path)), _BUILD_OPTIONS)

    assert _items(tmp_path) == [
        (0, "Child", "bar/foo/"),
        (0, "Deep", "bar/nested/foo/"),
        (0, "Root", "foo/"),
    ]


def test_serve_rebuilds_navigation_after_control_file_edit(
    tmp_path: Path,
) -> None:
    """The settled source dependency invalidates navigation during serve."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "other.md").write_text("# Other\n", encoding="utf-8")
    navigation = docs / ".nav.yml"
    navigation.write_text("nav: [index.md]\n", encoding="utf-8")
    config = _write_config(tmp_path)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("dev_addr: 127.0.0.1:0\n")
    log = (tmp_path / "serve.log").open("w+", encoding="utf-8")
    process = subprocess.Popen(  # noqa: S603
        [
            sys.executable,
            "-m",
            "zensical",
            "serve",
            "--config-file",
            str(config),
        ],
        cwd=tmp_path,
        stdout=log,
        stderr=subprocess.STDOUT,
    )

    def wait_for(condition: Callable[[], bool], timeout: float = 10.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if condition():
                return
            if process.poll() is not None:
                log.flush()
                log.seek(0)
                raise AssertionError(
                    f"serve exited with status {process.returncode}: "
                    f"{log.read()}"
                )
            time.sleep(0.02)
        log.flush()
        log.seek(0)
        raise AssertionError(f"serve did not rebuild navigation: {log.read()}")

    try:
        wait_for(lambda: _items_or_none(tmp_path) == [(0, "Home", "")])
        with navigation.open("r+", encoding="utf-8") as stream:
            stream.write("nav: [other.md]\n")
            stream.truncate()
        wait_for(lambda: _items_or_none(tmp_path) == [(0, "Other", "other/")])
        assert process.poll() is None
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        log.close()


def test_awesome_nav_replaces_literate_nav_and_rejects_extglobs(
    tmp_path: Path,
) -> None:
    """Upstream event ordering makes awesome-nav the final navigation owner."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "other.md").write_text("# Other\n", encoding="utf-8")
    (docs / "SUMMARY.md").write_text("* [Other](other.md)\n", encoding="utf-8")
    navigation = docs / ".nav.yml"
    navigation.write_text("nav: [index.md]\n", encoding="utf-8")
    config = _write_config(tmp_path, "awesome-nav\n  - literate-nav")

    zensical.build(str(config), _BUILD_OPTIONS)
    assert _items(tmp_path) == [(0, "Home", "")]

    navigation.write_text(
        "nav:\n  - '@(index.md|other.md)'\n", encoding="utf-8"
    )
    with pytest.raises(Exception, match="unsupported awesome-nav extglob"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_no_match_diagnostics_obey_strict_and_configured_levels(
    tmp_path: Path,
) -> None:
    """Warnings fail strict builds while an explicit info level does not."""
    docs = tmp_path / "docs"
    docs.mkdir()
    _write_template(tmp_path)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / ".nav.yml").write_text("nav: [missing.md]\n", encoding="utf-8")
    config = _write_config(tmp_path)

    with pytest.raises(Exception, match="awesome-nav reported errors"):
        zensical.build(str(config), {"clean": False, "strict": True})

    config = _write_config(
        tmp_path,
        "awesome-nav:\n      logs:\n        no_matches: info",
    )
    zensical.build(str(config), {"clean": False, "strict": True})
    assert _items(tmp_path) == []
