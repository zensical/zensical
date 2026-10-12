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

from __future__ import annotations

import subprocess
import sys
import time
from typing import TYPE_CHECKING, Any

import pytest
import yaml
from markdown import Markdown

import zensical

if TYPE_CHECKING:
    from pathlib import Path


def _write_config(
    root: Path, options: dict[str, Any], configuration: str, **settings: Any
) -> Path:
    """Configure includes through the plugin or the Markdown extension."""
    project = {"site_name": "Test", **settings}
    if configuration == "plugin":
        project["plugins"] = {"include-markdown": options}
    else:
        project["markdown_extensions"] = {
            "zensical.extensions.include_markdown": options
        }
        if configuration == "override":
            project["plugins"] = {
                "include-markdown": {
                    "opening_tag": "{!",
                    "closing_tag": "!}",
                    "directives": {"include": "other"},
                }
            }
    config = root / "mkdocs.yml"
    config.write_text(yaml.safe_dump(project), encoding="utf-8")
    return config


@pytest.fixture(
    params=[
        {},
        {
            "opening_tag": "[(",
            "closing_tag": ")]",
            "directives": {
                "include": "read+file",
                "include-markdown": "read.md",
            },
        },
    ],
    ids=["default", "custom"],
)
def syntax(request: pytest.FixtureRequest) -> dict[str, Any]:
    return request.param


@pytest.mark.parametrize("configuration", ["plugin", "extension", "override"])
def test_rebuilds_included_content_and_caches_other_pages(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    syntax: dict[str, Any],
    configuration: str,
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    opening = syntax.get("opening_tag", "{%")
    closing = syntax.get("closing_tag", "%}")
    directive = syntax.get("directives", {}).get(
        "include-markdown", "include-markdown"
    )
    (docs / "index.md").write_text(
        f'# Home\n\n{opening} {directive} "../shared.txt" {closing}\n',
        encoding="utf-8",
    )
    ordinary_markdown = "# Ordinary page\n\nContent without includes."
    if syntax:
        # The default syntax and similar names are inactive with custom tags.
        ordinary_markdown += (
            '\n\n{% include-markdown "unused.md" %}'
            '\n\n[( readXmd "unused.md" )]'
        )
    (docs / "ordinary.md").write_text(ordinary_markdown, encoding="utf-8")
    included = tmp_path / "shared.txt"
    included.write_text("First **inclusion**", encoding="utf-8")
    config = _write_config(tmp_path, syntax, configuration)
    ordinary_renders = []
    convert = Markdown.convert

    def record_render(md: Markdown, source: str) -> str:
        if source.strip() == ordinary_markdown:
            ordinary_renders.append(source)
        return convert(md, source)

    monkeypatch.setattr(Markdown, "convert", record_render)

    zensical.build(str(config), {"clean": False, "strict": False})

    output = tmp_path / "site" / "index.html"
    assert "First <strong>inclusion</strong>" in output.read_text(
        encoding="utf-8"
    )
    assert len(ordinary_renders) == 1

    # Only the included file is changed; the page and configuration are kept.
    included.write_text("Updated **inclusion**", encoding="utf-8")

    zensical.build(str(config), {"clean": False, "strict": False})

    html = output.read_text(encoding="utf-8")
    assert "Updated <strong>inclusion</strong>" in html
    assert "First <strong>inclusion</strong>" not in html
    assert len(ordinary_renders) == 1


def test_serve_rebuilds_multiline_include_tags(
    tmp_path: Path, syntax: dict[str, Any]
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    included = docs / "shared.txt"
    included.write_text("First inclusion", encoding="utf-8")
    opening = syntax.get("opening_tag", "{%")
    closing = syntax.get("closing_tag", "%}")
    directive = syntax.get("directives", {}).get("include", "include")
    (docs / "index.md").write_text(
        f'{opening}\n  {directive}\n  "shared.txt"\n{closing}', encoding="utf-8"
    )
    config = _write_config(tmp_path, syntax, "plugin", dev_addr="127.0.0.1:0")
    output = tmp_path / "site" / "index.html"

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

        def wait_for_content(expected: str) -> None:
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline and process.poll() is None:
                if output.exists() and expected in output.read_text(
                    encoding="utf-8"
                ):
                    return
                time.sleep(0.02)
            log.seek(0)
            raise AssertionError(
                f"Included content was not rebuilt: {log.read()}"
            )

        try:
            wait_for_content("First inclusion")

            # A source change must refresh the retained including page.
            included.write_text("Updated inclusion", encoding="utf-8")

            wait_for_content("Updated inclusion")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


@pytest.mark.parametrize("configuration", ["plugin", "extension"])
def test_disabled_includes_keep_pages_cached(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, configuration: str
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    source = '{% include-markdown "missing.md" %}'
    (docs / "index.md").write_text(source, encoding="utf-8")
    config = _write_config(tmp_path, {"enabled": False}, configuration)
    rendered = []
    convert = Markdown.convert

    def record_render(md: Markdown, content: str) -> str:
        if content.strip() == source:
            rendered.append(content)
        return convert(md, content)

    monkeypatch.setattr(Markdown, "convert", record_render)

    zensical.build(str(config), {"clean": False, "strict": False})
    zensical.build(str(config), {"clean": False, "strict": False})

    assert len(rendered) == 1
    assert source in (tmp_path / "site" / "index.html").read_text(
        encoding="utf-8"
    )


def test_converted_site_matches_plain_snippets(
    tmp_path: Path, syntax: dict[str, Any]
) -> None:
    docs = tmp_path / "docs"
    shared = docs / "shared"
    shared.mkdir(parents=True)
    opening = syntax.get("opening_tag", "{%")
    closing = syntax.get("closing_tag", "%}")
    directive = syntax.get("directives", {}).get("include", "include")
    index = docs / "index.md"
    outer = shared / "outer.txt"
    inner = shared / "inner.txt"
    index.write_text(
        f"# Home\n\n{{{{ 1 + 2 }}}}\n\n    {opening} {directive} "
        f'"./shared/outer.txt" {closing}',
        encoding="utf-8",
    )
    outer.write_text(
        f'    {opening} {directive} "./inner.txt" {closing}', encoding="utf-8"
    )
    inner.write_text(
        "    Included **content**\n\n    {{ 2 + 3 }}\n", encoding="utf-8"
    )
    options = {**syntax, "dedent": True, "preserve_includer_indent": False}
    config = _write_config(
        tmp_path,
        options,
        "plugin",
        markdown_extensions=["zensical.extensions.macros"],
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    output = tmp_path / "site" / "index.html"
    included_html = output.read_text(encoding="utf-8")
    assert "Included <strong>content</strong>" in included_html
    assert "{{ 2 + 3 }}" in included_html

    # Every tag, nested path, and mapped setting is converted before removal.
    index.write_text(
        '# Home\n\n{{ 1 + 2 }}\n\n--8<-- "shared/outer.txt:1"', encoding="utf-8"
    )
    outer.write_text('--8<-- "shared/inner.txt:1"', encoding="utf-8")
    project = {
        "site_name": "Test",
        "markdown_extensions": {
            "zensical.extensions.macros": {},
            "pymdownx.snippets": {
                "base_path": [str(docs)],
                "restrict_base_path": False,
                "check_paths": True,
                "url_download": True,
                "dedent_subsections": True,
            },
        },
    }
    config.write_text(yaml.safe_dump(project), encoding="utf-8")

    zensical.build(str(config), {"clean": False, "strict": False})

    assert output.read_text(encoding="utf-8") == included_html
