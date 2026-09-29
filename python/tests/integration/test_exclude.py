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

"""Integration tests for native mkdocs-exclude compatibility."""

from __future__ import annotations

import json
import subprocess
import sys
import time
from typing import TYPE_CHECKING, Any

import pytest
import yaml

import zensical

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path


def _project(
    root: Path, options: dict[str, Any], *, toml: bool = False
) -> Path:
    docs = root / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    overrides = root / "overrides"
    overrides.mkdir()
    if toml:
        config = root / "zensical.toml"
        config.write_text(
            "[project]\n"
            'site_name = "Exclude"\n'
            'site_url = "https://example.com/"\n'
            'extra_templates = ["export.html"]\n'
            "[project.theme]\n"
            'custom_dir = "overrides"\n'
            "[project.plugins.exclude]\n"
            + "\n".join(
                f"{key} = {json.dumps(value)}" for key, value in options.items()
            ),
            encoding="utf-8",
        )
    else:
        config = root / "mkdocs.yml"
        config.write_text(
            yaml.safe_dump(
                {
                    "site_name": "Exclude",
                    "site_url": "https://example.com/",
                    "dev_addr": "127.0.0.1:0",
                    "theme": {"custom_dir": "overrides"},
                    "extra_templates": ["export.html"],
                    "plugins": [{"exclude": options}],
                }
            ),
            encoding="utf-8",
        )
    return config


@pytest.mark.parametrize("toml", [False, True])
def test_excludes_pages_resources_and_extra_templates(
    tmp_path: Path, toml: bool
) -> None:
    config = _project(
        tmp_path,
        {
            "glob": ["drafts/*", "*/draft.md", "*.tmp", "export.html"],
            "regex": [r".*\.bin$", "private-"],
        },
        toml=toml,
    )
    docs = tmp_path / "docs"
    for name, content in {
        "guide/keep.md": "# Kept guide\n",
        "guide/private-note.md": "# Kept nested note\n",
        "guide/draft.md": "---\ninvalid: [\n---\n",
        "drafts/nested/page.md": "# Excluded subtree\n",
        "private-note.md": "# Excluded root note\n",
        "root.tmp": "temporary",
        "files/archive.tmp": "temporary",
        "files/uppercase.TMP": "case matters",
        "files/archive.bin": "binary",
        "export.html": "{{ must_not_render() }}",
    }.items():
        path = docs / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    # Exclusion covers theme assets and prevents an excluded docs override
    # from exposing the theme's version of the same file.
    (tmp_path / "overrides/root.tmp").write_text(
        "theme fallback", encoding="utf-8"
    )
    (tmp_path / "overrides/theme.tmp").write_text(
        "theme temporary", encoding="utf-8"
    )
    (tmp_path / "overrides/theme.txt").write_text(
        "theme asset", encoding="utf-8"
    )

    zensical.build(str(config), {"clean": False, "strict": True})

    site = tmp_path / "site"
    for name in (
        "guide/draft/index.html",
        "drafts/nested/page/index.html",
        "private-note/index.html",
        "root.tmp",
        "files/archive.tmp",
        "files/archive.bin",
        "export.html",
        "theme.tmp",
    ):
        assert not (site / name).exists(), name
    for name in (
        "index.html",
        "guide/keep/index.html",
        "guide/private-note/index.html",
        "files/uppercase.TMP",
        "theme.txt",
    ):
        assert (site / name).exists(), name

    # Excluded pages do not reach navigation, the search index or the sitemap.
    home = (site / "index.html").read_text()
    search = (site / "search.json").read_text()
    sitemap = (site / "sitemap.xml").read_text()
    for content in (home, search, sitemap):
        assert "drafts/nested/page" not in content
        assert "guide/draft" not in content
        assert "guide/keep/" in content
    assert "Excluded root note" not in home
    assert "Excluded root note" not in search


def test_regexes_preserve_python_match_semantics(tmp_path: Path) -> None:
    config = _project(
        tmp_path,
        {
            "regex": [
                r"(?i)draft\.md$",
                r"guide/(?!public\.)",
                r"([^/]+)/\1\.md$",
            ],
        },
    )
    docs = tmp_path / "docs"
    for name in (
        "DRAFT.md",
        "guide/private.md",
        "repeat/repeat.md",
        "guide/public.md",
        "nested/draft.md",
    ):
        path = docs / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("# Page\n", encoding="utf-8")

    zensical.build(str(config), {"clean": False, "strict": True})

    site = tmp_path / "site"
    for name in ("DRAFT", "guide/private", "repeat/repeat"):
        assert not (site / name / "index.html").exists()
    for name in ("guide/public", "nested/draft"):
        assert (site / name / "index.html").exists()


@pytest.mark.parametrize(
    "options",
    [{}, {"glob": None, "regex": None}, {"enabled": False, "glob": "*.md"}],
)
def test_empty_or_disabled_plugin_keeps_pages(
    tmp_path: Path, options: dict[str, Any]
) -> None:
    config = _project(tmp_path, options)

    zensical.build(str(config), {"clean": False, "strict": True})

    assert (tmp_path / "site/index.html").is_file()


@pytest.mark.parametrize(("pattern", "included"), [("", True), ([""], False)])
def test_empty_regex_scalar_and_list_have_different_meanings(
    tmp_path: Path, pattern: Any, included: bool
) -> None:
    config = _project(tmp_path, {"regex": pattern})

    zensical.build(str(config), {"clean": False, "strict": True})

    assert (tmp_path / "site/index.html").is_file() is included


def test_reports_invalid_glob(tmp_path: Path) -> None:
    config = _project(tmp_path, {"glob": "[bad"})

    with pytest.raises(RuntimeError, match=r"invalid exclude glob.*\[bad"):
        zensical.build(str(config), {"clean": False, "strict": True})


def test_preserves_hidden_awesome_nav_configuration(tmp_path: Path) -> None:
    config = _project(tmp_path, {"glob": ["*.yml", "draft.md"]})
    data = yaml.safe_load(config.read_text())
    data["plugins"].append("awesome-nav")
    config.write_text(yaml.safe_dump(data), encoding="utf-8")
    (tmp_path / "docs/.nav.yml").write_text(
        "nav:\n  - Custom home: index.md\n  - '*'\n", encoding="utf-8"
    )
    (tmp_path / "docs/draft.md").write_text(
        "# Excluded draft\n", encoding="utf-8"
    )
    (tmp_path / "docs/keep.md").write_text("# Visible page\n", encoding="utf-8")

    zensical.build(str(config), {"clean": False, "strict": True})

    home = (tmp_path / "site/index.html").read_text()
    assert "Custom home" in home
    assert "Visible page" in home
    assert "Excluded draft" not in home
    assert not (tmp_path / "site/draft/index.html").exists()


def test_serve_filters_new_files_and_removes_renamed_outputs(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, {"glob": ["*.excluded.md", "*.tmp"]})
    page = tmp_path / "docs/moving.md"
    asset = tmp_path / "docs/moving.txt"
    page.write_text("# Moving page\n", encoding="utf-8")
    asset.write_text("moving asset", encoding="utf-8")
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
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if condition():
                    return
                if process.poll() is not None:
                    break
                time.sleep(0.02)
            log.flush()
            log.seek(0)
            raise AssertionError(
                f"serve did not update excluded files: {log.read()}"
            )

        try:
            wait_for(
                lambda: (
                    (site / "moving/index.html").exists()
                    and (site / "moving.txt").exists()
                )
            )

            # Renaming into an excluded path retracts the existing outputs.
            page.rename(tmp_path / "docs/moving.excluded.md")
            asset.rename(tmp_path / "docs/moving.tmp")
            wait_for(
                lambda: (
                    not (site / "moving/index.html").exists()
                    and not (site / "moving.txt").exists()
                )
            )

            # New excluded inputs must stay unpublished after another revision.
            (tmp_path / "docs/new.excluded.md").write_text(
                "# Excluded new page\n", encoding="utf-8"
            )
            (tmp_path / "docs/new.tmp").write_text(
                "excluded new asset", encoding="utf-8"
            )
            (tmp_path / "docs/index.md").write_text(
                "# Updated home\n", encoding="utf-8"
            )
            wait_for(
                lambda: "Updated home" in (site / "index.html").read_text()
            )

            for name in (
                "moving.excluded/index.html",
                "moving.tmp",
                "new.excluded/index.html",
                "new.tmp",
            ):
                assert not (site / name).exists()
            assert "Moving page" not in (site / "search.json").read_text()

            # Reloading plugin configuration makes the retained files visible.
            data = yaml.safe_load(config.read_text())
            data["plugins"][0]["exclude"]["enabled"] = False
            config.write_text(yaml.safe_dump(data), encoding="utf-8")
            wait_for(
                lambda: (
                    (site / "moving.excluded/index.html").exists()
                    and (site / "new.excluded/index.html").exists()
                )
            )
            wait_for(
                lambda: (
                    (site / "moving.tmp").exists()
                    and (site / "new.tmp").exists()
                )
            )
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
