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

"""Integration tests for MkDocs-compatible HTML minification."""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
import time
from typing import TYPE_CHECKING

import pytest

import zensical

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path


def _project(root: Path, *, minify_html: bool = True) -> Path:
    """Create a project exercising HTML and inline language minification."""
    docs = root / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        """\
# Minify

<!-- remove this comment -->

<div pre>  preserved   text  </div>

<script>
  const value = { nested: { answer: 42 } };
  window.result = value?.nested?.answer ?? 0;
</script>

<script type="application/ld+json">
  { "preserved": true }
</script>

<style>
  @media screen and (min-width: 45em) {
    .probe { color: rgb(255, 0, 0); }
  }
</style>
""",
        encoding="utf-8",
    )
    config = root / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Minify
plugins:
  - minify:
      minify_html: {str(minify_html).lower()}
      minify_inline_js: true
      minify_inline_css: true
      htmlmin_opts:
        remove_comments: true
""",
        encoding="utf-8",
    )
    return config


def test_minifies_final_html_and_inline_languages(tmp_path: Path) -> None:
    """HTML runs last while script/style use parser-backed minifiers."""
    zensical.build(str(_project(tmp_path)), {"clean": False, "strict": False})
    output = (tmp_path / "site" / "index.html").read_text()

    assert "remove this comment" not in output
    assert "<div>  preserved   text  </div>" in output
    assert "const value={nested:{answer:42}};" in output
    assert "value?.nested?.answer??0" in output
    assert "screen and (min-width:45em)" in output
    assert "screen and(" not in output
    assert '{ "preserved": true }' in output


def test_inline_minification_does_not_require_html_minification(
    tmp_path: Path,
) -> None:
    """Inline-only mode retains the surrounding rendered HTML byte shape."""
    zensical.build(
        str(_project(tmp_path, minify_html=False)),
        {"clean": False, "strict": False},
    )
    output = (tmp_path / "site" / "index.html").read_text()

    assert "<!-- remove this comment -->" in output
    assert '<script type="application/ld+json">' in output
    assert "const value={nested:{answer:42}};" in output
    assert "screen and (min-width:45em)" in output


def _asset_project(root: Path, *, minify: bool, cache_safe: bool) -> Path:
    """Create a project covering exact, glob, and configured assets."""
    docs = root / "docs"
    (docs / "assets" / "nested").mkdir(parents=True)
    (docs / "styles").mkdir()
    (docs / "index.md").write_text("# Assets\n", encoding="utf-8")
    (docs / "assets" / "app.js").write_text(
        "const value = { answer: 42 };\nconsole.log(value.answer);\n",
        encoding="utf-8",
    )
    (docs / "assets" / "nested" / "worker.js").write_text(
        "self.addEventListener('message', (event) => { "
        "postMessage(event.data); });\n",
        encoding="utf-8",
    )
    (docs / "styles" / "site.css").write_text(
        ".card { color: rgb(255, 0, 0); padding: 0px 0px 0px 0px; }\n",
        encoding="utf-8",
    )
    (docs / "assets" / "unchanged.txt").write_text(
        "copied unchanged\n", encoding="utf-8"
    )
    config = root / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Asset minify
extra_javascript:
  - path: assets/app.js
    type: module
    defer: true
extra_css:
  - styles/site.css
plugins:
  - minify:
      minify_js: {str(minify).lower()}
      minify_css: {str(minify).lower()}
      cache_safe: {str(cache_safe).lower()}
      js_files:
        - assets/app.js
        - assets/**/*.js
      css_files: styles/*.css
""",
        encoding="utf-8",
    )
    return config


def _single_asset(root: Path, pattern: str) -> Path:
    """Return the single generated asset matching a glob."""
    paths = list((root / "site").glob(pattern))
    assert len(paths) == 1, paths
    return paths[0]


def test_minifies_hashes_and_rewrites_external_assets(tmp_path: Path) -> None:
    """Claimed assets are emitted once and projected into templates."""
    zensical.build(
        str(_asset_project(tmp_path, minify=True, cache_safe=True)),
        {"clean": False, "strict": False},
    )

    script = _single_asset(tmp_path, "assets/app.*.min.js")
    stylesheet = _single_asset(tmp_path, "styles/site.*.min.css")
    worker = _single_asset(tmp_path, "assets/nested/worker.*.min.js")
    assert not (tmp_path / "site" / "assets" / "app.js").exists()
    assert not (tmp_path / "site" / "styles" / "site.css").exists()
    assert "const value={answer:42};" in script.read_text()
    assert "color:red" in stylesheet.read_text()
    assert "postMessage(event.data)" in worker.read_text()

    for path in (script, stylesheet, worker):
        digest = hashlib.sha384(path.read_bytes()).hexdigest()[:6]
        assert f".{digest}.min." in path.name

    html = (tmp_path / "site" / "index.html").read_text()
    assert f'src="./{script.relative_to(tmp_path / "site")}"' in html
    assert f'href="./{stylesheet.relative_to(tmp_path / "site")}"' in html
    assert 'type="module"' in html
    assert re.search(r"<script[^>]+ defer(?:=| |>)", html)
    assert (tmp_path / "site" / "assets" / "unchanged.txt").read_text() == (
        "copied unchanged\n"
    )


def test_cache_safe_can_rename_without_minifying(tmp_path: Path) -> None:
    """Cache-safe naming is independent from content minification."""
    zensical.build(
        str(_asset_project(tmp_path, minify=False, cache_safe=True)),
        {"clean": False, "strict": False},
    )
    script = _single_asset(tmp_path, "assets/app.*.js")
    assert ".min.js" not in script.name
    assert "const value = { answer: 42 };" in script.read_text()
    digest = hashlib.sha384(script.read_bytes()).hexdigest()[:6]
    assert script.name == f"app.{digest}.js"


def test_external_minification_can_keep_original_names(tmp_path: Path) -> None:
    """Minification without cache-safe naming emits the upstream .min form."""
    zensical.build(
        str(_asset_project(tmp_path, minify=True, cache_safe=False)),
        {"clean": False, "strict": False},
    )
    script = tmp_path / "site" / "assets" / "app.min.js"
    assert script.is_file()
    assert "const value={answer:42};" in script.read_text()


def test_assets_support_an_absolute_site_directory(tmp_path: Path) -> None:
    """Physical output roots never enter logical asset identities."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Absolute output\n", encoding="utf-8")
    (docs / "app.js").write_text("const answer = 42;\n", encoding="utf-8")
    output = tmp_path / "absolute-output"
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Absolute output
site_dir: {output}
plugins:
  - minify:
      minify_js: true
      js_files: app.js
""",
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    assert (output / "index.html").is_file()
    assert (output / "app.min.js").read_text() == "const answer=42;"


def test_missing_explicit_asset_is_reported(tmp_path: Path) -> None:
    """An exact configured path remains an error as it is upstream."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Missing asset\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Missing asset
plugins:
  - minify:
      minify_js: true
      js_files: assets/missing.js
""",
        encoding="utf-8",
    )
    with pytest.raises(
        RuntimeError,
        match=r"selected asset does not exist: assets/missing\.js",
    ):
        zensical.build(str(config), {"clean": False, "strict": False})


def test_files_for_disabled_asset_kind_are_ignored(tmp_path: Path) -> None:
    """A selector is only active with its minifier or cache-safe mode."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# CSS only\n", encoding="utf-8")
    (docs / "site.css").write_text(
        ".card { color: rgb(255, 0, 0); }\n", encoding="utf-8"
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: CSS only
plugins:
  - minify:
      minify_css: true
      js_files: assets/missing.js
      css_files: site.css
""",
        encoding="utf-8",
    )
    zensical.build(str(config), {"clean": False, "strict": False})
    assert (tmp_path / "site" / "site.min.css").read_text() == (
        ".card{color:red}"
    )


def _unminified_asset_project(root: Path) -> Path:
    """Create colliding project/theme assets without enabling minify."""
    docs = root / "docs"
    overrides = root / "overrides"
    (docs / "assets").mkdir(parents=True)
    (overrides / "assets").mkdir(parents=True)
    (docs / "index.md").write_text("# Assets\n", encoding="utf-8")
    (docs / "assets" / "shared.txt").write_text("project\n", encoding="utf-8")
    (overrides / "assets" / "shared.txt").write_text(
        "theme\n", encoding="utf-8"
    )
    config = root / "mkdocs.yml"
    config.write_text(
        """\
site_name: Unminified assets
theme:
  name: material
  custom_dir: overrides
""",
        encoding="utf-8",
    )
    return config


def test_disabled_minify_uses_project_over_theme_precedence(
    tmp_path: Path,
) -> None:
    """The copy path consumes the same effective-resource relation."""
    config = _unminified_asset_project(tmp_path)
    zensical.build(str(config), {"clean": False, "strict": False})
    assert (tmp_path / "site" / "assets" / "shared.txt").read_text() == (
        "project\n"
    )


def test_disabled_minify_reconciles_asset_handoffs_and_removals(
    tmp_path: Path,
) -> None:
    """Serve reveals theme fallbacks and removes outputs without stale files."""
    config = _unminified_asset_project(tmp_path)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("dev_addr: 127.0.0.1:0\n")

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
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    output = tmp_path / "site" / "assets" / "shared.txt"

    def wait_for(condition: Callable[[], bool], timeout: float = 10.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if condition():
                return
            if process.poll() is not None:
                raise AssertionError(
                    f"serve exited with status {process.returncode}"
                )
            time.sleep(0.02)
        current = output.read_text() if output.is_file() else None
        raise AssertionError(
            f"serve did not reconcile the expected asset: {current!r}"
        )

    try:
        wait_for(lambda: output.is_file() and output.read_text() == "project\n")
        (tmp_path / "docs" / "assets" / "shared.txt").unlink()
        wait_for(lambda: output.is_file() and output.read_text() == "theme\n")
        (tmp_path / "overrides" / "assets" / "shared.txt").unlink()
        wait_for(lambda: not output.exists())
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def test_serve_retracts_superseded_cache_safe_assets(tmp_path: Path) -> None:
    """A changed asset removes its old hash and refreshes template paths."""
    config = _asset_project(tmp_path, minify=True, cache_safe=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("dev_addr: 127.0.0.1:0\n")

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
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    def wait_for(condition: Callable[[], bool], timeout: float = 10.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if condition():
                return
            if process.poll() is not None:
                raise AssertionError(
                    f"serve exited with status {process.returncode}"
                )
            time.sleep(0.02)
        raise AssertionError("serve did not produce the expected asset state")

    try:
        outputs = tmp_path / "site" / "assets"
        wait_for(lambda: len(list(outputs.glob("app.*.min.js"))) == 1)
        previous = next(outputs.glob("app.*.min.js"))
        source = tmp_path / "docs" / "assets" / "app.js"
        source.write_text(
            "const value = { answer: 43 };\nconsole.log(value.answer);\n",
            encoding="utf-8",
        )

        wait_for(
            lambda: (
                len(list(outputs.glob("app.*.min.js"))) == 1
                and not previous.exists()
                and previous.name
                not in (tmp_path / "site" / "index.html").read_text()
            )
        )
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
