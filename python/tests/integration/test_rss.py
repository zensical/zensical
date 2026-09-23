# Copyright (c) 2025-2026 Zensical and contributors
# SPDX-License-Identifier: MIT

"""Feed output and page-selection compatibility tests."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from pathlib import Path

import pytest
import zensical


def _build(root: Path, config: str) -> Path:
    (root / "mkdocs.yml").write_text(config, encoding="utf-8")
    zensical.build(str(root / "mkdocs.yml"), {"clean": False, "strict": False})
    return root / "site"


def _titles(path: Path) -> list[str]:
    root = ET.parse(path).getroot()
    return [item.findtext("title") or "" for item in root.findall("./channel/item")]


def test_created_updated_feeds_use_metadata_and_final_page_routes(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "---\ntitle: Home & News\ndate:\n  created: 2024-01-01\n"
        "  updated: 2024-04-01\ntags: [R&D]\n---\n# Home\n\nIntro.\n",
        encoding="utf-8",
    )
    (docs / "post.md").write_text(
        "---\ntitle: Post\ndate:\n  created: 2024-03-01\n"
        "  updated: 2024-03-02\nimage: logo.png\nrss:\n  feed_description: Feed only\n"
        "---\n# Post\n\nBody.\n",
        encoding="utf-8",
    )
    (docs / "logo.png").write_bytes(b"\x89PNG\r\n\x1a\n")
    (docs / "draft.md").write_text(
        "---\ndraft: true\ndate:\n  created: 2025-01-01\n---\n# Draft\n",
        encoding="utf-8",
    )
    site = _build(
        tmp_path,
        """site_name: Feed test
site_description: Site description
site_url: https://example.org/docs/
plugins:
  - rss:
      use_git: false
      categories: [tags]
      comments_path: '#comments'
      url_parameters: {utm_source: rss}
      date_from_meta:
        as_creation: date.created
        as_update: date.updated
        default_timezone: Europe/Paris
""",
    )
    assert _titles(site / "feed_rss_created.xml") == ["Post", "Home & News"]
    assert _titles(site / "feed_rss_updated.xml") == ["Home & News", "Post"]
    root = ET.parse(site / "feed_rss_created.xml").getroot()
    home = root.findall("./channel/item")[1]
    assert home.findtext("category") == "R&D"
    assert home.findtext("pubDate") == "Mon, 01 Jan 2024 00:00:00 +0100"
    post = root.findall("./channel/item")[0]
    assert post.findtext("link") == "https://example.org/docs/post/?utm_source=rss"
    assert post.findtext("comments") == "https://example.org/docs/post/#comments"
    assert post.find("enclosure").attrib == {
        "url": "https://example.org/docs/logo.png",
        "type": "image/png",
        "length": "8",
    }
    created = json.loads((site / "feed_json_created.json").read_text("utf-8"))
    assert created["items"][0]["url"] == "https://example.org/docs/post/?utm_source=rss"
    assert created["items"][0]["content_html"] == "Feed only"
    assert len(created["items"]) == 2
    assert (site / "rss.xsl").is_file()


def test_multiple_instances_filter_and_retract_separate_feeds(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    (docs / "blog").mkdir(parents=True)
    (docs / "index.md").write_text(
        "---\ndate: 2024-01-01\n---\n# Home\n", encoding="utf-8"
    )
    (docs / "blog" / "one.md").write_text(
        "---\ndate: 2024-02-01\n---\n# One\n", encoding="utf-8"
    )
    config = """site_name: Feed test
site_description: Site description
site_url: https://example.org/
plugins:
  - rss:
      use_git: false
      date_from_meta: {as_creation: date, as_update: date}
  - rss:
      use_git: false
      date_from_meta: {as_creation: date, as_update: date}
      match_path: blog/.*
      feeds_filenames:
        rss_created: blog.xml
        rss_updated: blog-updated.xml
        json_created: blog.json
        json_updated: blog-updated.json
"""
    site = _build(tmp_path, config)
    assert _titles(site / "feed_rss_created.xml") == ["One", "Home"]
    assert _titles(site / "blog.xml") == ["One"]
    assert json.loads((site / "blog.json").read_text("utf-8"))["items"][0][
        "title"
    ] == "One"
    assert (site / "rss.xsl").is_file()
    (docs / "blog" / "one.md").unlink()
    _build(tmp_path, config)
    assert _titles(site / "blog.xml") == []


def test_git_dates_follow_renamed_page_history(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    original = docs / "old.md"
    original.write_text("# Original\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "docs/old.md"], check=True)
    author = {"GIT_AUTHOR_NAME": "Test", "GIT_AUTHOR_EMAIL": "test@example.org",
              "GIT_COMMITTER_NAME": "Test", "GIT_COMMITTER_EMAIL": "test@example.org"}
    subprocess.run(
        ["git", "-C", str(tmp_path), "-c", "commit.gpgsign=false", "commit", "-qm", "add"], check=True,
        env={**os.environ, **author, "GIT_AUTHOR_DATE": "2024-01-01T12:00:00+00:00",
             "GIT_COMMITTER_DATE": "2024-01-01T12:00:00+00:00"},
    )
    subprocess.run(
        ["git", "-C", str(tmp_path), "mv", "docs/old.md", "docs/new.md"],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(tmp_path), "-c", "commit.gpgsign=false", "commit", "-qm", "rename"], check=True,
        env={**os.environ, **author, "GIT_AUTHOR_DATE": "2024-03-01T12:00:00+00:00",
             "GIT_COMMITTER_DATE": "2024-03-01T12:00:00+00:00"},
    )
    site = _build(tmp_path, """site_name: Git feed
site_description: Git dates
site_url: https://example.org/
plugins: [rss]
""")
    items = json.loads((site / "feed_json_created.json").read_text("utf-8"))["items"]
    assert len(items) == 1
    assert items[0]["date_published"] == "2024-01-01T12:00:00+00:00"
    assert items[0]["date_modified"] == "2024-03-01T12:00:00+00:00"


def test_yaml_timestamp_keeps_explicit_offset(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "---\ndate: 2024-01-02T03:04:05+02:00\n---\n# Offset\n",
        encoding="utf-8",
    )
    site = _build(tmp_path, """site_name: Offset feed
site_description: Offset feed
site_url: https://example.org/
plugins:
  - rss:
      use_git: false
      date_from_meta: {as_creation: date, as_update: date, default_timezone: UTC}
""")
    item = json.loads((site / "feed_json_created.json").read_text("utf-8"))["items"][0]
    assert item["date_published"] == "2024-01-02T03:04:05+02:00"


def test_incompatible_match_pattern_is_reported(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    with pytest.raises(Exception, match="invalid rss match_path"):
        _build(tmp_path, """site_name: Pattern feed
site_url: https://example.org/
plugins:
  - rss: {match_path: '(?=index)'}
""")


def test_blog_author_names_and_generated_social_cards(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    posts = docs / "blog" / "posts"
    posts.mkdir(parents=True)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "blog" / ".authors.yml").write_text(
        "authors:\n  jane:\n    name: Jane Doe\n    email: jane@example.org\n    description: Writer\n"
        "    avatar: jane.png\n", encoding="utf-8",
    )
    (posts / "one.md").write_text(
        "---\ndate: 2024-01-01\nauthors: [jane]\n---\n# One\n",
        encoding="utf-8",
    )
    layouts = tmp_path / "layouts"
    layouts.mkdir()
    (layouts / "plain.yml").write_text(
        "tags:\n  og:image: '{{ image.url }}'\n"
        "size: { width: 320, height: 168 }\n"
        "layers:\n  - background: { color: '#123456' }\n",
        encoding="utf-8",
    )
    site = _build(tmp_path, """site_name: Blog feed
site_description: Blog dates
site_url: https://example.org/
theme: {name: material}
plugins:
  - blog:
      authors: true
      archive: false
      categories: false
  - social:
      cache: false
      cards_layout: plain
  - rss:
      use_git: false
      match_path: blog/posts/.*
      date_from_meta: {as_creation: date, as_update: date}
""")
    item = ET.parse(site / "feed_rss_created.xml").getroot().find(
        "./channel/item"
    )
    assert item is not None
    assert item.findtext("author") == "jane@example.org (Jane Doe)"
    assert item.findtext("link") == "https://example.org/blog/2024/01/01/one/"
    enclosure = item.find("enclosure")
    assert enclosure is not None
    assert enclosure.attrib["url"].endswith("assets/images/social/blog/2024/01/01/one.png")
    assert int(enclosure.attrib["length"]) > 0


def test_serve_updates_and_retracts_feed_entries(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "---\ndate: 2024-01-01\n---\n# Home\n", encoding="utf-8"
    )
    post = docs / "post.md"
    post.write_text("---\ndate: 2024-02-01\n---\n# Post\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text("""site_name: Live feed
site_description: Live feed
site_url: https://example.org/
dev_addr: 127.0.0.1:0
plugins:
  - rss:
      use_git: false
      date_from_meta: {as_creation: date, as_update: date}
""", encoding="utf-8")
    log = (tmp_path / "serve.log").open("w+", encoding="utf-8")
    process = subprocess.Popen(  # noqa: S603
        [sys.executable, "-m", "zensical", "serve", "--config-file", str(config)],
        cwd=tmp_path, stdout=log, stderr=subprocess.STDOUT,
    )
    feed = tmp_path / "site" / "feed_rss_created.xml"

    def wait_for(titles: list[str]) -> None:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if feed.is_file():
                try:
                    if _titles(feed) == titles:
                        return
                except ET.ParseError:
                    pass
            if process.poll() is not None:
                break
            time.sleep(0.02)
        log.flush()
        log.seek(0)
        raise AssertionError(f"feed did not become {titles}: {log.read()}")

    try:
        wait_for(["Post", "Home"])
        post.write_text(
            "---\ndate: 2024-02-01\n---\n# Revised\n", encoding="utf-8"
        )
        wait_for(["Revised", "Home"])
        post.unlink()
        wait_for(["Home"])
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        log.close()
