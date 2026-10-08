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


"""Test MkDocs file inclusion settings across builds and previews."""

from __future__ import annotations

import json
import subprocess
import sys
import time
from typing import TYPE_CHECKING, Any

import pytest
import yaml

import zensical
from zensical.config import ConfigurationError

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path


_TEMPLATE = """\
{{ page.content }}
<nav>{% for item in nav.items recursive %}
<a href="{{ item.url }}">{{ item.title }}</a>
{% if item.children %}{{ loop(item.children) }}{% endif %}
{% endfor %}</nav>
"""


def _project(
    root: Path, files: dict[str, str], *, toml: bool = False, **settings: Any
) -> Path:
    """Create a project with observable content and navigation."""
    for name, content in {"index.md": "# Home\n", **files}.items():
        path = root / "docs" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    overrides = root / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(_TEMPLATE, encoding="utf-8")
    config = {
        "site_name": "Inclusion",
        "site_url": "https://example.com/",
        "dev_addr": "127.0.0.1:0",
        **settings,
    }
    if toml:
        path = root / "zensical.toml"
        text = "[project]\n" + "\n".join(
            f"{key} = {json.dumps(value)}" for key, value in config.items()
        )
        text += '\n[project.theme]\ncustom_dir = "overrides"\n'
    else:
        path = root / "mkdocs.yml"
        config["theme"] = {"custom_dir": "overrides"}
        text = yaml.safe_dump(config)
    path.write_text(text, encoding="utf-8")
    return path


def _build(config: Path) -> None:
    zensical.build(str(config), {"clean": False, "strict": True})


def _nav(site: Path) -> str:
    return (
        (site / "index.html")
        .read_text()
        .split("<nav>", 1)[1]
        .split("</nav>", 1)[0]
    )


@pytest.mark.parametrize("toml", [False, True])
@pytest.mark.parametrize(
    "setting", ["exclude_docs", "draft_docs", "not_in_nav"]
)
def test_gitignore_patterns_and_negations(
    tmp_path: Path, toml: bool, setting: str
) -> None:
    # Anchors, recursive paths, escaped prefixes and ordered negations
    # are exercised together.
    excluded = [
        "root.md",
        "nested/secret.md",
        "secret.md",
        "drafts/deep/page.md",
        "guide/old.md",
        "#private.md",
        "!private.md",
    ]
    included = [
        "nested/root.md",
        "drafts/keep.md",
        "guide/new.md",
        "nested/guide/old.md",
        "keep.md",
    ]
    patterns = r"""# comment
/root.md
secret.md
drafts/
!drafts/keep.md
/guide/[a-z]*.md
!/guide/new.md
\#private.md
\!private.md
"""
    config = _project(
        tmp_path,
        {name: f"# {name}\n" for name in excluded + included},
        toml=toml,
        **{setting: patterns},
    )

    _build(config)

    site = tmp_path / "site"
    nav = _nav(site)
    search = json.loads((site / "search.json").read_text())
    sitemap = (site / "sitemap.xml").read_text()
    for name in included + excluded:
        output = site / name.removesuffix(".md") / "index.html"
        assert output.is_file() == (
            name in included or setting == "not_in_nav"
        ), name
        assert (f">{name}</a>" in nav) == (name in included), name
    # Unlisted pages are included in search and the sitemap.
    assert ("nested/secret/" in json.dumps(search)) == (setting == "not_in_nav")
    assert ("nested/secret/" in sitemap) == (setting == "not_in_nav")


@pytest.mark.parametrize("restore", [False, True])
def test_default_exclusions_can_be_negated(
    tmp_path: Path, restore: bool
) -> None:
    files = {
        ".hidden.md": "# Hidden",
        ".assets/logo.svg": "asset",
        "nested/.hidden/page.md": "# Nested hidden",
        "templates/page.md": "# Template",
        "templates/data.txt": "data",
        "nested/templates/page.md": "# Ordinary page",
    }
    config = _project(
        tmp_path, files, exclude_docs="!.*\n!/templates/" if restore else None
    )

    _build(config)

    site = tmp_path / "site"
    for output in [
        ".hidden/index.html",
        ".assets/logo.svg",
        "nested/.hidden/page/index.html",
        "templates/page/index.html",
        "templates/data.txt",
    ]:
        assert (site / output).is_file() == restore, output
    assert (site / "nested/templates/page/index.html").is_file()


@pytest.mark.parametrize("setting", ["exclude_docs", "draft_docs"])
def test_exclusions_cover_resources_and_templates_before_processing(
    tmp_path: Path, setting: str
) -> None:
    files = {
        "broken.md": "---\ninvalid: [\n---",
        "private.txt": "private",
        "export.html": "{{ must_not_render() }}",
    }
    config = _project(
        tmp_path,
        files,
        extra_templates=["export.html"],
        **{setting: "broken.md\n*.txt\nexport.html"},  # ty:ignore[invalid-argument-type]
    )
    (tmp_path / "overrides/private.txt").write_text("theme fallback")
    (tmp_path / "overrides/theme.txt").write_text("theme asset")

    _build(config)

    site = tmp_path / "site"
    for output in ["broken/index.html", "private.txt", "export.html"]:
        assert not (site / output).exists(), output
    assert (site / "theme.txt").read_text() == "theme asset"


def test_explicit_navigation_can_include_unlisted_pages(tmp_path: Path) -> None:
    config = _project(
        tmp_path,
        {"hidden.md": "# Unlisted"},
        not_in_nav="hidden.md",
        nav=[{"Home": "index.md"}, {"Explicit title": "hidden.md"}],
    )

    _build(config)

    assert 'href="hidden/">Explicit title</a>' in _nav(tmp_path / "site")


def test_exclude_plugin_can_filter_explicitly_included_hidden_pages(
    tmp_path: Path,
) -> None:
    files = {
        ".hidden/keep.md": "# Kept",
        ".hidden/drop.md": "---\ninvalid: [\n---",
    }
    config = _project(
        tmp_path,
        files,
        exclude_docs="!.hidden/",
        plugins=[{"exclude": {"glob": "*/drop.md"}}],
    )

    _build(config)

    assert (tmp_path / "site/.hidden/keep/index.html").is_file()
    assert not (tmp_path / "site/.hidden/drop/index.html").exists()


def test_exclusions_take_precedence_and_control_files_remain_available(
    tmp_path: Path,
) -> None:
    files = {
        ".nav.yml": "nav:\n  - Custom home: index.md\n  - '*'\n",
        "private.md": "---\ninvalid: [\n---",
        "draft.md": "# Draft",
        "unlisted.md": "# Unlisted",
    }
    config = _project(
        tmp_path,
        files,
        exclude_docs="private.md",
        draft_docs="private.md\ndraft.md",
        not_in_nav="!private.md\n!draft.md\nunlisted.md",
        plugins=["awesome-nav"],
    )

    _build(config)

    site = tmp_path / "site"
    assert "Custom home" in _nav(site)
    assert not (site / "private/index.html").exists()
    assert not (site / "draft/index.html").exists()
    assert (site / "unlisted/index.html").is_file()


@pytest.mark.parametrize(
    "setting", ["exclude_docs", "draft_docs", "not_in_nav"]
)
@pytest.mark.parametrize("value", [True, 42, ["*.md"], {"pattern": "*.md"}])
def test_rejects_non_string_patterns(
    tmp_path: Path, setting: str, value: Any
) -> None:
    config = _project(tmp_path, {}, **{setting: value})

    with pytest.raises(ConfigurationError, match=setting):
        _build(config)


@pytest.mark.parametrize(
    "setting", ["exclude_docs", "draft_docs", "not_in_nav"]
)
def test_reports_invalid_patterns(tmp_path: Path, setting: str) -> None:
    config = _project(tmp_path, {}, **{setting: "!"})  # ty:ignore[invalid-argument-type]

    with pytest.raises(ValueError, match=setting):
        _build(config)


def test_serve_includes_marked_drafts_and_updates_inclusion(
    tmp_path: Path,
) -> None:
    files = {
        "draft.md": "# Draft page",
        "draft.txt": "draft asset",
        "private.md": "# Private",
        "moving.md": "# Moving",
        "moving.txt": "moving asset",
        "unlisted.md": "# Unlisted",
        ".included/page.md": "# Hidden page",
        ".included/data.txt": "hidden asset",
    }
    config = _project(
        tmp_path,
        files,
        exclude_docs="private.md\n*.excluded.md\n*.excluded.txt\n!.included/",
        draft_docs="draft.*",
        not_in_nav="unlisted.md",
    )
    site = tmp_path / "site"

    with (tmp_path / "serve.log").open("w+", encoding="utf-8") as log:
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

        def wait_for(condition: Callable[[], bool]) -> None:
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                if condition():
                    return
                if process.poll() is not None:
                    break
                time.sleep(0.02)
            log.flush()
            log.seek(0)
            raise AssertionError(
                f"serve did not update file inclusion: {log.read()}"
            )

        try:
            wait_for(
                lambda: (
                    (site / "draft/index.html").exists()
                    and (site / "index.html").exists()
                    and (site / "moving.txt").exists()
                )
            )

            # Drafts are rendered with a marker
            # and are omitted from inferred navigation.
            assert (
                'class="mkdocs-draft-marker zensical-draft-marker"'
                in (site / "draft/index.html").read_text()
            )
            assert (site / "draft.txt").read_text() == "draft asset"
            assert "Draft page" not in _nav(site)
            assert "Unlisted" not in _nav(site)
            assert not (site / "private/index.html").exists()
            wait_for(
                lambda: (
                    (site / ".included/page/index.html").exists()
                    and (site / ".included/data.txt").exists()
                )
            )

            # Hidden directories are discovered
            # when they are created during preview.
            nested = tmp_path / "docs/.included/.new"
            nested.mkdir()
            (nested / "page.md").write_text("# New hidden page")
            wait_for((site / ".included/.new/page/index.html").exists)

            # Renamed hidden directories retain their pages and assets.
            (tmp_path / "docs/.included").rename(tmp_path / "docs/visible")
            wait_for(
                lambda: (
                    (site / "visible/page/index.html").exists()
                    and not (site / ".included/page/index.html").exists()
                )
            )

            # Outputs are removed when their sources
            # are renamed into excluded paths.
            (tmp_path / "docs/moving.md").rename(
                tmp_path / "docs/moving.excluded.md"
            )
            (tmp_path / "docs/moving.txt").rename(
                tmp_path / "docs/moving.excluded.txt"
            )
            wait_for(
                lambda: (
                    not (site / "moving/index.html").exists()
                    and not (site / "moving.txt").exists()
                )
            )

            # Updated drafts are included in subsequent revisions
            # with the same marker.
            (tmp_path / "docs/draft.md").write_text("# Updated draft")
            wait_for(
                lambda: (
                    "Updated draft" in (site / "draft/index.html").read_text()
                )
            )
            assert (
                'class="mkdocs-draft-marker zensical-draft-marker"'
                in (site / "draft/index.html").read_text()
            )
            assert not (site / "moving.excluded/index.html").exists()
            assert not (site / "moving.excluded.txt").exists()

            # Changed patterns are reloaded and
            # the previously excluded sources are published.
            settings = yaml.safe_load(config.read_text())
            settings.update(exclude_docs=None, draft_docs=None, not_in_nav=None)
            config.write_text(yaml.safe_dump(settings))
            wait_for(
                lambda: (
                    (site / "private/index.html").exists()
                    and (site / "moving.excluded.txt").exists()
                    and "Updated draft" in _nav(site)
                )
            )
            assert (
                'class="mkdocs-draft-marker zensical-draft-marker"'
                not in (site / "draft/index.html").read_text()
            )
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

    # A build after preview excludes drafts even when the render cache is warm.
    settings.update(draft_docs="draft.*")
    config.write_text(yaml.safe_dump(settings))
    _build(config)

    assert not (site / "draft/index.html").exists()
    assert not (site / "draft.txt").exists()
