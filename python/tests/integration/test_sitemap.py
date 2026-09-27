# Copyright (c) 2025-2026 Zensical and contributors
#
# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

"""Integration tests for sitemap coverage of published pages."""

from __future__ import annotations

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


@pytest.mark.parametrize("explicit_nav", [False, True])
def test_sitemap_includes_pages_outside_navigation(
    tmp_path: Path, explicit_nav: bool
) -> None:
    """Posts and unlisted pages still appear in the sitemap."""
    docs = tmp_path / "docs"
    posts = docs / "blog" / "posts"
    posts.mkdir(parents=True)
    (docs / "index.md").write_text("# Home\n")
    (docs / "guide.md").write_text("# Guide\n")
    (docs / "blog" / "index.md").write_text("# Blog\n")
    (posts / "sample.md").write_text(
        "---\ndate: 2020-01-02\ncategories:\n  - Notes\n---\n\n# Sample post\n"
    )
    nav = 'nav = [{ "Home" = "index.md" }]\n' if explicit_nav else ""
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_url = "https://docs.example.test/"\n'
        f'site_name = "Sitemap test"\n{nav}\n[project.plugins.blog]\n'
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    site = tmp_path / "site"
    locations = set(
        re.findall(r"<loc>([^<]+)</loc>", (site / "sitemap.xml").read_text())
    )
    assert locations == {
        "https://docs.example.test/",
        "https://docs.example.test/guide/",
        "https://docs.example.test/blog/",
        "https://docs.example.test/blog/archive/2020/",
        "https://docs.example.test/blog/category/notes/",
        "https://docs.example.test/blog/2020/01/02/sample-post/",
    }
    assert site.joinpath("blog/2020/01/02/sample-post/index.html").is_file()


def test_sitemap_tracks_unlisted_posts_during_serve(tmp_path: Path) -> None:
    """Adding or removing a post updates the sitemap without changing nav."""
    docs = tmp_path / "docs"
    posts = docs / "blog" / "posts"
    posts.mkdir(parents=True)
    (docs / "index.md").write_text("# Home\n")
    (docs / "blog" / "index.md").write_text("# Blog\n")
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_url = "https://docs.example.test/"\n'
        'site_name = "Sitemap test"\n'
        'nav = [{ "Home" = "index.md" }]\n\n'
        "[project.plugins.blog]\n"
    )

    log = (tmp_path / "serve.log").open("w+", encoding="utf-8")
    process = subprocess.Popen(  # noqa: S603
        [
            sys.executable,
            "-m",
            "zensical",
            "serve",
            "--config-file",
            str(config),
            "--dev-addr",
            "127.0.0.1:0",
        ],
        cwd=tmp_path,
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    sitemap = tmp_path / "site" / "sitemap.xml"
    post_url = "https://docs.example.test/blog/2020/01/02/sample-post/"

    def contains_post() -> bool:
        try:
            return post_url in sitemap.read_text()
        except OSError:
            return False

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
        raise AssertionError(f"sitemap did not update: {log.read()}")

    try:
        wait_for(sitemap.is_file)
        post = posts / "sample.md"
        post.write_text("---\ndate: 2020-01-02\n---\n\n# Sample post\n")
        wait_for(contains_post)
        post.unlink()
        wait_for(lambda: not contains_post())
    finally:
        process.terminate()
        process.wait(timeout=5)
        log.close()
