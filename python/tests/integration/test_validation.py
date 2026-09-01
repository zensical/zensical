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
import threading
import time
from typing import TYPE_CHECKING

import zensical

if TYPE_CHECKING:
    from collections.abc import Callable
    from io import TextIOWrapper
    from pathlib import Path

    import pytest


def _wait_for(
    condition: Callable[[], bool],
    process: subprocess.Popen[str],
    output: Callable[[], str],
    *,
    timeout: float = 10.0,
) -> None:
    """Wait for a serve-process condition or report its captured output."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return
        if process.poll() is not None:
            raise AssertionError(
                f"Serve process exited with {process.returncode}:\n{output()}"
            )
        time.sleep(0.02)
    raise AssertionError(f"Serve process timed out:\n{output()}")


def _collect_output(stream: TextIOWrapper, lines: list[str]) -> None:
    """Collect process output without blocking its pipes."""
    lines.extend(stream)


def test_validation_reports_issues_after_rendering(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Validation reports source and link issues after page rendering."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Hello\n\n[normal](missing.md)\n\n[autoref][missing-id]\n",
        encoding="utf-8",
    )
    (docs / "other.md").write_text("# Other\n", encoding="utf-8")
    (tmp_path / "watched.md").write_text(
        "# Watched support file\n", encoding="utf-8"
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"
watch = ["watched.md"]

[project.validation]
unresolved_references = true
""".lstrip(),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "page does not exist" in captured.err
    assert "unresolved link reference" in captured.err
    assert "2 issues found" in captured.err
    assert captured.err.count("2 issues found") == 1

    (docs / "index.md").write_text("# Hello\n", encoding="utf-8")
    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "No issues found" in captured.err
    assert "2 issues found" not in captured.err


def test_validation_reports_unresolved_autorefs(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Autorefs that fail to resolve are reported as invalid links."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Hello\n\n[normal](missing.md)\n\n[autoref][missing-id]\n",
        encoding="utf-8",
    )
    (docs / "other.md").write_text("# Other\n", encoding="utf-8")
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"

[project.plugins.autorefs]
""".lstrip(),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "page does not exist" in captured.err
    assert "unresolved autoref" in captured.err
    assert "2 issues found" in captured.err
    assert captured.err.count("2 issues found") == 1

    (docs / "index.md").write_text("# Hello\n", encoding="utf-8")
    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "No issues found" in captured.err
    assert "2 issues found" not in captured.err


def test_validation_reports_references_resolved_by_autorefs(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """The deprecated unresolved_references setting keeps its original
    behavior, reporting references even when autorefs resolve them."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Home\n\n[Target][target-heading]\n", encoding="utf-8"
    )
    (docs / "other.md").write_text(
        "# Other\n\n## target-heading\n", encoding="utf-8"
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"

[project.plugins.autorefs]

[project.validation]
unresolved_references = true
""".lstrip(),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "unresolved link reference" in captured.err
    assert "unresolved autoref" not in captured.err
    assert "1 issue found" in captured.err
    output = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert 'href="other/#target-heading"' in output


def test_validation_refreshes_cached_autoref_resolutions(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Validation refreshes autorefs when targets are added and removed."""
    docs = tmp_path / "docs"
    docs.mkdir()
    identifier = "validation-target-only-available-after-rebuild"
    (docs / "index.md").write_text(
        f"# Home\n\n[Target][{identifier}]\n", encoding="utf-8"
    )
    target = docs / "other.md"
    target.write_text("# Other\n", encoding="utf-8")
    stable_identifier = "validation-stable-target"
    stable_marker = "validation-stable-cache-sentinel"
    (docs / "stable.md").write_text(
        (
            f"# Stable\n\n[{stable_marker}][{stable_identifier}]\n\n"
            f"## {stable_identifier}\n"
        ),
        encoding="utf-8",
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"

[project.plugins.autorefs]
""".lstrip(),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "unresolved autoref" in captured.err
    marker = stable_marker.encode()

    def stable_cache_entries() -> dict[str, bytes]:
        cache_dir = tmp_path / ".cache"
        return {
            path.name: data
            for path in cache_dir.iterdir()
            if path.is_file() and marker in (data := path.read_bytes())
        }

    stable_cache = stable_cache_entries()
    assert stable_cache

    target.write_text(f"# Other\n\n## {identifier}\n", encoding="utf-8")
    zensical.build(str(config), {"clean": False, "strict": False})

    captured = capfd.readouterr()
    assert "No issues found" in captured.err
    assert "unresolved autoref" not in captured.err
    output = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert f'href="other/#{identifier}"' in output
    assert stable_cache_entries() == stable_cache

    target.write_text("# Other\n", encoding="utf-8")
    zensical.build(str(config), {"clean": False, "strict": False})

    captured = capfd.readouterr()
    assert "unresolved autoref" in captured.err
    output = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert f'href="other/#{identifier}"' not in output
    assert stable_cache_entries() == stable_cache


def test_serve_refreshes_autorefs_before_validation(tmp_path: Path) -> None:
    """Serving resolves a fixed autoref without reporting a stale snapshot."""
    docs = tmp_path / "docs"
    docs.mkdir()
    identifier = "serve-target-only-available-after-rebuild"
    (docs / "index.md").write_text(
        f"# Home\n\n[Target][{identifier}]\n", encoding="utf-8"
    )
    target = docs / "other.md"
    target.write_text("# Other\n", encoding="utf-8")
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"
dev_addr = "127.0.0.1:0"

[project.plugins.autorefs]
""".lstrip(),
        encoding="utf-8",
    )

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
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    assert process.stdout is not None
    assert process.stderr is not None
    stdout: list[str] = []
    stderr: list[str] = []
    threads = [
        threading.Thread(
            target=_collect_output,
            args=(process.stdout, stdout),
            daemon=True,
        ),
        threading.Thread(
            target=_collect_output,
            args=(process.stderr, stderr),
            daemon=True,
        ),
    ]
    for thread in threads:
        thread.start()

    def output() -> str:
        return "".join(stdout + stderr)

    try:
        _wait_for(lambda: "1 issue found" in "".join(stderr), process, output)
        offset = len(stderr)

        target.write_text(f"# Other\n\n## {identifier}\n", encoding="utf-8")
        _wait_for(
            lambda: "No issues found" in "".join(stderr[offset:]),
            process,
            output,
        )

        rebuild_output = "".join(stderr[offset:])
        assert "unresolved autoref" not in rebuild_output
        rendered = tmp_path / "site" / "index.html"
        _wait_for(
            lambda: (
                rendered.exists()
                and f'href="other/#{identifier}"'
                in rendered.read_text(encoding="utf-8")
            ),
            process,
            output,
        )
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        for thread in threads:
            thread.join(timeout=1)


def test_cached_template_autorefs_refresh_with_targets(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Autorefs introduced by cached templates use current target data."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    target = docs / "other.md"
    target.write_text("# Other\n", encoding="utf-8")

    identifier = "template-autoref-target"
    overrides = tmp_path / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(
        (
            "<main>{{ page.content }}</main>"
            f'<autoref identifier="{identifier}">Target</autoref>'
        ),
        encoding="utf-8",
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"

[project.theme]
custom_dir = "overrides"

[project.plugins.autorefs]
""".lstrip(),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})
    output = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert f'href="other/#{identifier}"' not in output

    # Template autorefs cannot be located in the Markdown source, so they
    # are reported as page-level issues without a source location
    captured = capfd.readouterr()
    assert f"unresolved autoref `{identifier}` in index.md" in captured.err
    assert f"unresolved autoref `{identifier}` in other.md" in captured.err
    assert "2 issues found" in captured.err

    target.write_text(f"# Other\n\n## {identifier}\n", encoding="utf-8")
    zensical.build(str(config), {"clean": False, "strict": False})
    output = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert f'href="other/#{identifier}"' in output

    captured = capfd.readouterr()
    assert "No issues found" in captured.err


def test_cached_template_refreshes_when_page_autoref_changes(
    tmp_path: Path,
) -> None:
    """Page-local autoref facts participate in the template cache key."""
    docs = tmp_path / "docs"
    docs.mkdir()
    source = docs / "index.md"
    source.write_text(
        "# Home\n\n[First title][first-target]\n", encoding="utf-8"
    )
    (docs / "other.md").write_text(
        "# Other\n\n## first-target\n\n## second-target\n",
        encoding="utf-8",
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"

[project.plugins.autorefs]
""".lstrip(),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})
    output = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert 'href="other/#first-target">First title</a>' in output

    # Both references occupy page-local slot zero. Only their cached facts
    # distinguish the template inputs after the Markdown pass.
    source.write_text(
        "# Home\n\n[Second title][second-target]\n", encoding="utf-8"
    )
    zensical.build(str(config), {"clean": False, "strict": False})
    output = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert 'href="other/#second-target">Second title</a>' in output
    assert 'href="other/#first-target">First title</a>' not in output
