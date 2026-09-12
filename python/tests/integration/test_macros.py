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

"""Integration coverage for macros page selection and verbose output."""

from __future__ import annotations

import json
import subprocess
import sys
from typing import TYPE_CHECKING

import pytest
import yaml

import zensical

if TYPE_CHECKING:
    from pathlib import Path


def _write_config(
    root: Path, options: dict[str, str | bool], *, toml: bool = False
) -> Path:
    (root / "content").mkdir(exist_ok=True)
    overrides = root / "overrides"
    overrides.mkdir(exist_ok=True)
    (overrides / "main.html").write_text("{{ page.content }}", encoding="utf-8")
    if toml:
        config = root / "zensical.toml"
        content = (
            '[project]\nsite_name = "Macros"\ndocs_dir = "content"\n'
            '[project.theme]\ncustom_dir = "overrides"\n'
            "[project.plugins.macros]\n"
        )
        content += "\n".join(
            f"{name} = {json.dumps(value)}" for name, value in options.items()
        )
    else:
        config = root / "mkdocs.yml"
        content = yaml.safe_dump(
            {
                "site_name": "Macros",
                "docs_dir": "content",
                "theme": {"custom_dir": "overrides"},
                "plugins": {"macros": options},
            }
        )
    config.write_text(content, encoding="utf-8")
    return config


@pytest.mark.parametrize("toml", [False, True], ids=["yaml", "toml"])
def test_force_render_paths_uses_docs_paths_and_page_metadata(
    tmp_path: Path, toml: bool
) -> None:
    config = _write_config(
        tmp_path,
        {
            "render_by_default": False,
            "force_render_paths": (
                "/guides/\n!guides/drafts/\nguides/drafts/keep.md"
            ),
        },
        toml=toml,
    )
    pages = {
        "index.md": (None, False),
        "guides/index.md": (None, True),
        "guides/disabled.md": (False, False),
        "guides/drafts/page.md": (None, False),
        "guides/drafts/keep.md": (None, True),
        "outside.md": (True, True),
        "nested/guides/page.md": (None, False),
    }
    for path, (override, _) in pages.items():
        page = tmp_path / "content" / path
        page.parent.mkdir(parents=True, exist_ok=True)
        header = (
            ""
            if override is None
            else f"---\nrender_macros: {str(override).lower()}\n---\n"
        )
        page.write_text(header + "Value: {{ 1 + 1 }}\n", encoding="utf-8")

    zensical.build(str(config), {"clean": True, "strict": False})

    for path, (_, rendered) in pages.items():
        output = tmp_path / "site" / path.removesuffix(".md")
        output = (
            output.with_suffix(".html")
            if output.name == "index"
            else output / "index.html"
        )
        html = output.read_text(encoding="utf-8")
        assert ("Value: 2" if rendered else "Value: {{ 1 + 1 }}") in html, path


def test_force_render_paths_changes_between_builds(tmp_path: Path) -> None:
    for index, (pattern, rendered) in enumerate(
        [("", False), ("/index.md", True), ("!index.md", False)]
    ):
        config = _write_config(
            tmp_path,
            {
                "render_by_default": False,
                "force_render_paths": pattern,
            },
        )
        if index == 0:
            (tmp_path / "content" / "index.md").write_text(
                "Value: {{ 1 + 1 }}\n", encoding="utf-8"
            )
        zensical.build(str(config), {"clean": index == 0, "strict": False})
        html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        assert ("Value: 2" if rendered else "Value: {{ 1 + 1 }}") in html


def test_verbose_chatter_reaches_cli_and_can_be_disabled(
    tmp_path: Path,
) -> None:
    for index, verbose in enumerate([False, True, False]):
        config = _write_config(
            tmp_path, {"verbose": verbose, "on_error_fail": True}
        )
        if index == 0:
            (tmp_path / "content" / "index.md").write_text(
                '{{ greet("World") }}\n', encoding="utf-8"
            )
            (tmp_path / "main.py").write_text(
                "def define_env(env):\n"
                '    chatter = env.start_chatting("Example", color="cyan")\n'
                '    chatter("Registered macros")\n'
                "    @env.macro\n"
                "    def greet(name):\n"
                '        chatter("Greeting:", name)\n'
                '        return "Hello " + name\n',
                encoding="utf-8",
            )
        result = subprocess.run(  # noqa: S603
            [sys.executable, "-m", "zensical.main", "build", "-f", str(config)],
            capture_output=True,
            text=True,
            check=True,
        )
        html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        assert "Hello World" in html
        if verbose:
            assert "[macros - Example] - Registered macros" in result.stderr
            assert "[macros - Example] - Greeting: World" in result.stderr
            assert "Rendering page: index.md" in result.stderr
            assert "Loading local module:" in result.stderr
        else:
            assert "[macros -" not in result.stderr
        assert "[macros -" not in result.stdout
        assert "[macros -" not in html
