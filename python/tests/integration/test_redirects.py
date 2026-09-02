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

"""Integration tests for MkDocs-compatible redirect artifacts."""

from __future__ import annotations

import subprocess
import sys
import time
from typing import TYPE_CHECKING, Any

import pytest

import zensical

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def _write_project(root: Path, redirect_maps: str) -> Path:
    """Create a small project with internal and external redirect targets."""
    docs = root / "docs"
    (docs / "guide").mkdir(parents=True)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "new.md").write_text("# New\n", encoding="utf-8")
    (docs / "guide" / "topic.md").write_text(
        "# Topic\n\n## Details\n", encoding="utf-8"
    )
    config = root / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Redirects
plugins:
  - redirects:
      redirect_maps:
{redirect_maps}
""",
        encoding="utf-8",
    )
    return config


def test_redirects_generate_mkdocs_compatible_artifacts(tmp_path: Path) -> None:
    """Internal, fragment, and external targets use the upstream paths."""
    config = _write_project(
        tmp_path,
        """\
        old.md: new.md
        legacy/deep.md: guide/topic.md#details
        external.md: https://example.com/new?q=1
""",
    )
    zensical.build(str(config), _BUILD_OPTIONS)

    old = (tmp_path / "site" / "old" / "index.html").read_text()
    nested = (tmp_path / "site" / "legacy" / "deep" / "index.html").read_text()
    external = (tmp_path / "site" / "external" / "index.html").read_text()
    assert '<link rel="canonical" href="../new/">' in old
    assert '<link rel="canonical" href="../../guide/topic/#details">' in nested
    assert (
        '<link rel="canonical" href="https://example.com/new?q=1">' in external
    )
    assert "noindex" not in old


def test_redirects_without_directory_urls_write_html_files(
    tmp_path: Path,
) -> None:
    """File-style URLs retain MkDocs' relative target calculation."""
    config = _write_project(tmp_path, "        old.md: new.md\n")
    with config.open("a", encoding="utf-8") as file:
        file.write("use_directory_urls: false\n")
    zensical.build(str(config), _BUILD_OPTIONS)

    old = (tmp_path / "site" / "old.html").read_text()
    assert '<link rel="canonical" href="new.html">' in old


def test_missing_redirect_target_warns_and_strict_mode_fails(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Missing targets are omitted and retain MkDocs strict semantics."""
    config = _write_project(tmp_path, "        old.md: missing.md\n")
    zensical.build(str(config), _BUILD_OPTIONS)
    assert not (tmp_path / "site" / "old" / "index.html").exists()
    assert (
        "Redirect target 'missing.md' does not exist!" in capfd.readouterr().err
    )

    with pytest.raises(RuntimeError, match="strict flag is set"):
        zensical.build(str(config), {"clean": False, "strict": True})


@pytest.mark.parametrize(
    ("kind", "message"),
    [("page", "collides with a page"), ("asset", "documentation asset")],
)
def test_redirect_output_collisions_are_rejected(
    tmp_path: Path, kind: str, message: str
) -> None:
    """No concurrent producer may own a configured redirect output."""
    config = _write_project(tmp_path, "        old.md: new.md\n")
    if kind == "page":
        (tmp_path / "docs" / "old.md").write_text("# Existing\n")
    else:
        asset = tmp_path / "docs" / "old" / "index.html"
        asset.parent.mkdir()
        asset.write_text("existing asset", encoding="utf-8")

    with pytest.raises(RuntimeError, match=message):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_unsafe_redirect_source_is_rejected(tmp_path: Path) -> None:
    """Redirect outputs cannot escape the site directory."""
    config = _write_project(tmp_path, "        ../old.md: new.md\n")
    with pytest.raises(RuntimeError, match="not a safe relative path"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_invalid_source_suffix_warns_but_still_generates(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Upstream's source warning does not suppress a valid redirect."""
    config = _write_project(
        tmp_path, "        old.txt: https://example.com/new\n"
    )
    zensical.build(str(config), _BUILD_OPTIONS)
    assert (tmp_path / "site" / "old" / "index.html").is_file()
    assert "'old.txt' is not a valid markdown file" in capfd.readouterr().err


def test_duplicate_redirect_outputs_are_rejected(tmp_path: Path) -> None:
    """Different source names cannot resolve to one generated file."""
    config = _write_project(
        tmp_path,
        """\
        foo.md: new.md
        foo/index.md: new.md
""",
    )
    with pytest.raises(RuntimeError, match="configured more than once"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_redirect_output_cannot_replace_a_static_template(
    tmp_path: Path,
) -> None:
    """The upstream post-build overwrite becomes a deterministic error."""
    config = _write_project(tmp_path, "        404.md: new.md\n")
    with config.open("a", encoding="utf-8") as file:
        file.write("use_directory_urls: false\n")
    with pytest.raises(RuntimeError, match="rendered template"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_repeated_build_removes_and_restores_redirect_with_its_target(
    tmp_path: Path,
) -> None:
    """An internal target controls ownership across non-clean builds."""
    config = _write_project(tmp_path, "        old.md: new.md\n")
    target = tmp_path / "docs" / "new.md"
    output = tmp_path / "site" / "old" / "index.html"

    zensical.build(str(config), _BUILD_OPTIONS)
    assert output.is_file()

    target.unlink()
    zensical.build(str(config), _BUILD_OPTIONS)
    assert not output.exists()

    target.write_text("# New again\n", encoding="utf-8")
    zensical.build(str(config), _BUILD_OPTIONS)
    assert output.is_file()


def test_serve_removes_and_restores_redirect_with_its_target(
    tmp_path: Path,
) -> None:
    """One retained workflow reconciles a disappearing internal target."""
    config = _write_project(tmp_path, "        old.md: new.md\n")
    with config.open("a", encoding="utf-8") as file:
        file.write("dev_addr: 127.0.0.1:0\n")

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
    target = tmp_path / "docs" / "new.md"
    output = tmp_path / "site" / "old" / "index.html"
    target_output = tmp_path / "site" / "new" / "index.html"

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
        raise AssertionError(
            f"serve did not reconcile the redirect output: {log.read()}"
        )

    try:
        wait_for(lambda: output.is_file() and target_output.is_file())
        target.unlink()
        wait_for(lambda: not output.exists())
        target.write_text("# New again\n", encoding="utf-8")
        wait_for(
            lambda: (
                output.is_file()
                and target_output.is_file()
                and "New again" in target_output.read_text(encoding="utf-8")
            )
        )
        assert process.poll() is None
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        log.close()
