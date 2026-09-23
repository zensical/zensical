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

"""Integration tests for MkDocs-compatible mkdocstrings artifacts."""

from __future__ import annotations

import builtins
from typing import TYPE_CHECKING, Any

import pytest
from yaml import safe_dump

import zensical
from zensical.compat import mkdocstrings

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def test_object_inventory_is_restored_from_cache(tmp_path: Path) -> None:
    """The cached inventory is published when no handler updates it."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    config = tmp_path / "zensical.toml"
    config.write_text('[project]\nsite_name = "Inventory"\n', encoding="utf-8")

    cache = tmp_path / ".cache"
    cache.mkdir()
    inventory = b"cached object inventory"
    (cache / "objects.inv").write_bytes(inventory)

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (tmp_path / "site" / "objects.inv").read_bytes() == inventory
    assert (cache / "objects.inv").read_bytes() == inventory


@pytest.mark.parametrize("enabled", [False, True])
def test_autorefs_without_backlinks_does_not_load_handlers(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, enabled: bool
) -> None:
    """Ordinary autorefs must work without the backlink dependencies."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        '# Home\n\n<autoref identifier="home">Home</autoref>\n',
        encoding="utf-8",
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_name = "No backlinks"\n'
        '[project.markdown_extensions."zensical.extensions.autorefs"]\n'
        f"enabled = {str(enabled).lower()}\n",
        encoding="utf-8",
    )
    original_import = builtins.__import__

    def guarded_import(name: str, *args: Any, **kwargs: Any) -> Any:
        if name in {"mkdocstrings", "mkdocs_autorefs"}:
            raise AssertionError(
                f"Backlinks are disabled: unexpected import of {name}"
            )
        return original_import(name, *args, **kwargs)

    monkeypatch.setattr(builtins, "__import__", guarded_import)

    # Check both a fresh build and reuse of the cached Markdown and templates.
    for _ in range(2):
        zensical.build(str(config), _BUILD_OPTIONS)

        content = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        if enabled:
            assert (
                '<a class="autorefs autorefs-internal" href="#home">Home</a>'
                in content
            )
        else:
            assert '<autoref identifier="home">Home</autoref>' in content
        assert "zensical:autoref" not in content
        assert not (tmp_path / ".cache" / "mkdocstrings").exists()


@pytest.mark.parametrize("backlinks", [None, False, "flat", "tree"])
@pytest.mark.parametrize("blog", [False, True])
def test_backlinks_across_cold_cached_and_changed_builds(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    backlinks: str | bool | None,
    blog: bool,
) -> None:
    """Backlinks are opt-in and refresh when only a referring page changes."""
    pytest.importorskip("mkdocstrings_handlers.python")
    docs = tmp_path / "docs"
    docs.mkdir()
    overrides = tmp_path / "overrides"
    overrides.mkdir()
    # The final HTML pass must resolve template references alongside the
    # backlink placeholders produced by the handler in the page content.
    (overrides / "main.html").write_text(
        "{{ page.content }}"
        "<autoref identifier='sample.Target'>Template reference</autoref>",
        encoding="utf-8",
    )
    (tmp_path / "sample.py").write_text(
        'class Target:\n    """A documented target."""\n', encoding="utf-8"
    )
    (docs / "api.md").write_text(
        "# API\n\n::: sample.Target\n", encoding="utf-8"
    )
    if blog:
        posts = docs / "blog" / "posts"
        posts.mkdir(parents=True)
        (docs / "blog" / "index.md").write_text("# Journal\n", encoding="utf-8")
        (overrides / "blog-post.html").write_text(
            "{{ page.content }}", encoding="utf-8"
        )
        guide = posts / "guide.md"
        prefix = "---\ndate: 2026-09-03\n---\n"
        guide_url = "../blog/2026/09/03/guide/"
    else:
        guide = docs / "guide.md"
        prefix = ""
        guide_url = "../guide/"
    guide.write_text(
        prefix + "# Guide\n\n## Example\n\n[Target][sample.Target]\n",
        encoding="utf-8",
    )

    handler_options: dict[str, Any] = {
        "show_root_heading": True,
        "show_source": False,
    }
    if backlinks is not None:
        handler_options["backlinks"] = backlinks
    plugins: dict[str, dict[str, Any]] = {
        "mkdocstrings": {
            "handlers": {
                "python": {"paths": ["."], "options": handler_options}
            },
        },
    }
    if blog:
        plugins["blog"] = {
            "authors": False,
            "archive": False,
            "categories": False,
        }
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        safe_dump(
            {
                "site_name": "Backlinks",
                "theme": {"custom_dir": "overrides"},
                "plugins": plugins,
            }
        ),
        encoding="utf-8",
    )

    def unexpected_backlink_work(*_args: Any, **_kwargs: Any) -> Any:
        raise AssertionError("This build must not load or render backlinks")

    # Omitted and false options must never cross the backlink Python bridge.
    if not backlinks:
        monkeypatch.setattr(
            mkdocstrings, "get_backlink_aliases", unexpected_backlink_work
        )
        monkeypatch.setattr(
            mkdocstrings, "render_backlinks", unexpected_backlink_work
        )

    zensical.build(str(config), _BUILD_OPTIONS)

    api = tmp_path / "site" / "api" / "index.html"
    first = api.read_text(encoding="utf-8")
    assert "<backlinks" not in first
    assert "<autoref" not in first
    assert 'href="./#sample.Target">Template reference</a>' in first
    assert "zensical:autoref" not in first
    if backlinks:
        assert 'class="doc doc-backlinks"' in first
        assert f"{guide_url}#example" in first
    else:
        assert 'class="doc doc-backlinks"' not in first
        assert not (tmp_path / ".cache" / "mkdocstrings").exists()

    # A warm build must reuse rendered backlinks without initializing handlers.
    with monkeypatch.context() as warm:
        warm.setattr(mkdocstrings, "_get_handlers", unexpected_backlink_work)
        zensical.build(str(config), _BUILD_OPTIONS)

    assert api.read_text(encoding="utf-8") == first

    # Keep the API page cached while changing the backlink title and anchor.
    guide.write_text(
        prefix + "# Guide\n\n## Updated\n\n[Target][sample.Target]\n",
        encoding="utf-8",
    )
    zensical.build(str(config), _BUILD_OPTIONS)

    updated = api.read_text(encoding="utf-8")
    if backlinks:
        assert f"{guide_url}#updated" in updated
        assert f"{guide_url}#example" not in updated
    else:
        assert updated == first
        assert not (tmp_path / ".cache" / "mkdocstrings").exists()


@pytest.mark.parametrize("in_template", [False, True])
def test_backlinks_inside_autoref_titles(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, in_template: bool
) -> None:
    """Nested backlinks survive Markdown caching and template parsing."""
    pytest.importorskip("mkdocstrings_handlers.python")
    docs = tmp_path / "docs"
    docs.mkdir()
    overrides = tmp_path / "overrides"
    overrides.mkdir()

    nested = (
        '<autoref identifier="sample.Target">Template '
        '<backlinks identifier="sample.Target" handler="python" />'
        '</autoref>'
    )
    (overrides / "main.html").write_text(
        "{{ page.content }}" + (nested if in_template else ""),
        encoding="utf-8",
    )
    (tmp_path / "sample.py").write_text(
        'class Target:\n    """A documented target."""\n', encoding="utf-8"
    )
    (docs / "api.md").write_text(
        "# API\n\n::: sample.Target\n\n" + ("" if in_template else nested),
        encoding="utf-8",
    )
    (docs / "guide.md").write_text(
        "# Guide\n\n[Target][sample.Target]\n", encoding="utf-8"
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        safe_dump(
            {
                "site_name": "Nested replacements",
                "theme": {"custom_dir": "overrides"},
                "plugins": {
                    "mkdocstrings": {
                        "handlers": {
                            "python": {
                                "paths": ["."],
                                "options": {
                                    "backlinks": "flat",
                                    "show_root_heading": True,
                                },
                            },
                        },
                    },
                },
            }
        ),
        encoding="utf-8",
    )

    # Use an inline fragment so the nested result is valid content for a link.
    fragment = '<span class="backlink-test">Linked from Guide</span>'
    monkeypatch.setattr(mkdocstrings, "render_backlinks", lambda *_: fragment)

    zensical.build(str(config), _BUILD_OPTIONS)

    api = tmp_path / "site" / "api" / "index.html"
    output = api.read_text(encoding="utf-8")
    assert f">Template {fragment}</a>" in output
    assert "<backlinks" not in output
    assert "<autoref" not in output
    assert "zensical:autoref" not in output

    def unexpected_backlink_work(*_args: Any, **_kwargs: Any) -> Any:
        raise AssertionError("The warm build must use cached backlinks")

    # Cached fragments must also be retained inside a reference title.
    monkeypatch.setattr(mkdocstrings, "_get_handlers", unexpected_backlink_work)
    monkeypatch.setattr(
        mkdocstrings, "render_backlinks", unexpected_backlink_work
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert api.read_text(encoding="utf-8") == output
