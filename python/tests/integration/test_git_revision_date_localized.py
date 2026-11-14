# Copyright (c) 2025-2026 Zensical and contributors
# SPDX-License-Identifier: MIT
# ruff: noqa: S603, S607  # Fixed Git commands are run in temporary repositories.

"""Exercise revision dates with real Git history and retained builds."""

from __future__ import annotations

import os
import subprocess
import sys
import time
from typing import TYPE_CHECKING

import pytest
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


def git(root: Path, *args: str, date: str | None = None) -> str:
    """Run Git with a fixed identity and optionally distinct author dates."""
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": "Writer",
        "GIT_AUTHOR_EMAIL": "writer@example.org",
        "GIT_COMMITTER_NAME": "Builder",
        "GIT_COMMITTER_EMAIL": "builder@example.org",
    }
    if date:
        env["GIT_AUTHOR_DATE"] = date
        env["GIT_COMMITTER_DATE"] = "2025-01-01T00:00:00Z"
    return subprocess.check_output(
        [
            "git",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            f"core.hooksPath={os.devnull}",
            "-C",
            str(root),
            *args,
        ],
        env=env,
        text=True,
    ).strip()


def repository(root: Path) -> Path:
    """Create a repository with a tracked documentation page."""
    root.mkdir(parents=True, exist_ok=True)
    git(root, "init", "-q")
    docs = root / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Home\n\n{{ git_revision_date_localized }}\n\n"
        "{{ git_creation_date_localized }}\n",
        encoding="utf-8",
    )
    git(root, "add", "docs")
    git(root, "commit", "-qm", "Create page", date="2024-01-01T00:00:00Z")
    return docs


def build(
    root: Path, options: str = "", *, strict: bool = False
) -> BeautifulSoup:
    """Build a site and expose the raw metadata through a custom template."""
    overrides = root / "overrides"
    overrides.mkdir(exist_ok=True)
    (overrides / "main.html").write_text(
        """{% if page.meta is defined %}{{ page.content | safe }}
{% for prefix in ['git_revision_date_localized', 'git_creation_date_localized', 'git_site_revision_date_localized'] %}
<data id="{{ prefix }}">{{ page.meta[prefix ~ '_raw_iso_datetime'] }}</data>
<data id="{{ prefix }}-hash">{{ page.meta[prefix ~ '_hash'] }}</data>
<data id="{{ prefix }}-tag">{{ page.meta[prefix ~ '_tag'] }}</data>
<data id="{{ prefix }}-custom">{{ page.meta[prefix ~ '_raw_custom'] }}</data>
{% endfor %}
{% endif %}
""",  # noqa: E501
        encoding="utf-8",
    )
    config = root / "mkdocs.yml"
    config.write_text(
        f"""site_name: Revision dates
theme:
  custom_dir: overrides
plugins:
  - git-revision-date-localized:
      type: iso_date
      enable_creation_date: true
{options}
""",
        encoding="utf-8",
    )
    zensical.build(str(config), {"clean": False, "strict": strict})
    return BeautifulSoup(
        (root / "site" / "index.html").read_text(encoding="utf-8"),
        "html.parser",
    )


def value(html: BeautifulSoup, kind: str = "revision", suffix: str = "") -> str:
    """Read one observable metadata value from the generated page."""
    return html.select_one(f"data#git_{kind}_date_localized{suffix}").get_text()  # ty:ignore[unresolved-attribute]


def test_author_dates_inline_placeholders_tags_and_persistent_cache(
    tmp_path: Path,
) -> None:
    docs = repository(tmp_path)
    page = docs / "index.md"
    page.write_text(
        page.read_text(encoding="utf-8") + "\nA substantive update.\n",
        encoding="utf-8",
    )
    git(tmp_path, "add", "docs")
    git(tmp_path, "commit", "-qm", "Update page", date="2024-02-02T12:34:56Z")
    git(tmp_path, "tag", "-a", "v1", "-m", "Release")

    html = build(tmp_path)

    assert value(html) == "2024-02-02 12:34:56"
    assert value(html, "creation") == "2024-01-01 00:00:00"
    assert value(html, suffix="-hash") == git(tmp_path, "rev-parse", "HEAD")
    assert value(html, suffix="-tag") == "v1"
    assert (
        html.select_one(
            ".git-revision-date-localized-plugin-iso_date"
        ).get_text()  # ty:ignore[unresolved-attribute]
        == "2024-02-02"
    )
    assert "{{ git_" not in html.get_text()

    # Uncommitted content is rendered
    # while committed dates and raw caches are retained.
    cache = tmp_path / ".cache" / "plugin" / "git-revision-date-localized"
    before = {path: path.stat().st_mtime_ns for path in cache.glob("*.json")}
    page.write_text(
        page.read_text(encoding="utf-8") + "\nUncommitted edit.\n",
        encoding="utf-8",
    )
    html = build(tmp_path)

    assert value(html) == "2024-02-02 12:34:56"
    assert "Uncommitted edit." in html.get_text()
    assert before
    assert before == {
        path: path.stat().st_mtime_ns for path in cache.glob("*.json")
    }


def test_renames_and_ignored_commits_preserve_creation_history(
    tmp_path: Path,
) -> None:
    docs = repository(tmp_path)
    git(tmp_path, "mv", "docs/index.md", "docs/old.md")
    git(tmp_path, "commit", "-qm", "Move page", date="2024-02-01T00:00:00Z")
    old = docs / "old.md"
    old.write_text(
        old.read_text(encoding="utf-8") + "\nUpdate.\n", encoding="utf-8"
    )
    git(tmp_path, "add", "docs")
    git(tmp_path, "commit", "-qm", "Update page", date="2024-03-01T00:00:00Z")
    ignored = git(tmp_path, "rev-parse", "HEAD")
    git(tmp_path, "mv", "docs/old.md", "docs/index.md")
    git(
        tmp_path,
        "commit",
        "-qm",
        "Move page again",
        date="2024-04-01T00:00:00Z",
    )

    html = build(tmp_path)

    assert value(html) == "2024-03-01 00:00:00"
    assert value(html, "creation") == "2024-01-01 00:00:00"

    html = build(
        tmp_path,
        "      enable_git_follow: false\n"
        "      enable_parallel_processing: false\n",
    )

    assert value(html) == "2024-04-01 00:00:00"
    assert value(html, "creation") == "2024-01-01 00:00:00"

    # Abbreviated hashes, comments, and blank lines
    # are accepted in the ignore file.
    ignore_file = tmp_path / ".git-blame-ignore-revs"
    ignore_file.write_text(
        f"# Formatting updates\n\n{ignored[:12]} # ignored\n", encoding="utf-8"
    )
    html = build(
        tmp_path, "      ignored_commits_file: .git-blame-ignore-revs\n"
    )

    assert value(html) == "2024-01-01 00:00:00"
    assert value(html, "creation") == "2024-01-01 00:00:00"

    # An ignore-file edit takes effect without a new Git commit.
    ignore_file.write_text("", encoding="utf-8")
    html = build(
        tmp_path, "      ignored_commits_file: .git-blame-ignore-revs\n"
    )

    assert value(html) == "2024-03-01 00:00:00"


def test_page_locale_timezone_custom_format_and_exclusion(
    tmp_path: Path,
) -> None:
    docs = repository(tmp_path)
    page = docs / "index.md"
    page.write_text(
        "---\nlocale: fr_FR\n---\n" + page.read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    html = build(
        tmp_path,
        "      timezone: America/New_York\n      locale: de\n"
        "      custom_format: '%d %B %Y at %H:%M'\n",
    )

    assert value(html) == "2023-12-31 19:00:00"
    assert value(html, suffix="-custom") == "31 décembre 2023 at 19:00"

    html = build(tmp_path, "      exclude: ['*.md']\n")

    assert value(html) == ""
    assert "{{ git_revision_date_localized }}" in html.get_text()


def test_shallow_history_is_invalidated_after_deepening(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "original"
    docs = repository(source)
    (docs / "index.md").write_text("# Updated\n", encoding="utf-8")
    git(source, "add", "docs")
    git(source, "commit", "-qm", "Update", date="2024-02-01T00:00:00Z")
    clone = tmp_path / "clone"
    git(tmp_path, "clone", "-q", "--depth=1", source.as_uri(), str(clone))
    head = git(clone, "rev-parse", "HEAD")

    html = build(clone)

    assert "shallow repository" in capfd.readouterr().err
    assert value(html, "creation") == "2024-02-01 00:00:00"

    git(clone, "fetch", "-q", "--unshallow")
    html = build(clone)

    assert git(clone, "rev-parse", "HEAD") == head
    assert value(html, "creation") == "2024-01-01 00:00:00"


def test_worktrees_and_source_repository_discovery(tmp_path: Path) -> None:
    original = tmp_path / "original"
    repository(original)
    worktree = tmp_path / "linked"
    git(original, "worktree", "add", "-q", "--detach", str(worktree), "HEAD")

    html = build(worktree)

    assert value(html) == "2024-01-01 00:00:00"

    # The physical target's nested repository
    # is used through a documentation symlink.
    nested = tmp_path / "nested"
    repository(nested)
    (nested / "docs" / "index.md").write_text("# Nested\n", encoding="utf-8")
    git(nested, "add", "docs")
    git(nested, "commit", "-qm", "Nested update", date="2024-06-01T00:00:00Z")
    (worktree / "docs" / "index.md").unlink()
    (worktree / "docs" / "index.md").symlink_to(nested / "docs" / "index.md")

    html = build(worktree)

    assert value(html) == "2024-06-01 00:00:00"
    assert value(html, "site_revision") == "2024-01-01 00:00:00"


def test_untracked_pages_and_missing_repository_fallback(
    tmp_path: Path,
) -> None:
    tracked = tmp_path / "tracked"
    tracked.mkdir()
    git(tracked, "init", "-q")
    (tracked / "marker.txt").write_text("Tracked\n", encoding="utf-8")
    git(tracked, "add", "marker.txt")
    git(
        tracked,
        "commit",
        "-qm",
        "Initialize repository",
        date="2024-01-01T00:00:00Z",
    )
    (tracked / "docs").mkdir()
    (tracked / "docs" / "index.md").write_text(
        "# Untracked\n", encoding="utf-8"
    )

    html = build(tracked)

    assert value(html) == value(html, "creation")
    assert value(html, suffix="-hash") == ""

    # A repository is absent in the sibling project.
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "docs").mkdir()
    (outside / "docs" / "index.md").write_text(
        "# No repository\n", encoding="utf-8"
    )
    with pytest.raises(RuntimeError, match="fallback_to_build_date"):
        build(outside)

    html = build(
        outside,
        "      fallback_to_build_date: true\n      strict: false\n",
        strict=True,
    )

    assert value(html) == value(html, "creation")
    assert value(html, suffix="-hash") == ""


def test_timeago_assets_and_print_fallback(tmp_path: Path) -> None:
    repository(tmp_path)

    html = build(tmp_path, "      type: timeago\n")

    assert (
        html.select_one(".timeago")["datetime"] == "2024-01-01T00:00:00+00:00"  # ty:ignore[not-subscriptable]
    )
    assert (
        html.select_one(
            ".git-revision-date-localized-plugin-iso_date"
        ).get_text()  # ty:ignore[unresolved-attribute]
        == "2024-01-01"
    )
    assert "Intl.RelativeTimeFormat" in (
        tmp_path / "site/js/git-revision-date-localized.js"
    ).read_text(encoding="utf-8")
    assert "@media print" in (
        tmp_path / "site/css/git-revision-date-localized.css"
    ).read_text(encoding="utf-8")


def test_default_theme_footer_and_literal_unicode_source_paths(
    tmp_path: Path,
) -> None:
    docs = repository(tmp_path)
    page = docs / "révision[1].md"
    page.write_text("# Literal source path\n", encoding="utf-8")
    git(tmp_path, "add", "docs")
    git(
        tmp_path,
        "commit",
        "-qm",
        "Add literal path",
        date="2024-07-01T00:00:00Z",
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        "site_name: Native dates\nplugins:\n  - git-revision-date-localized:\n"
        "      type: iso_date\n      enable_creation_date: true\n",
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    html = BeautifulSoup(
        (tmp_path / "site" / "révision[1]" / "index.html").read_text(
            encoding="utf-8"
        ),
        "html.parser",
    )
    footer = html.select_one(".md-source-file")
    assert footer is not None
    assert footer.get_text().count("2024-07-01") == 2


@pytest.mark.parametrize(
    ("options", "message"),
    [
        ("      timezone: No/Such_Zone\n", "timezone"),
        ("      locale: 'invalid locale'\n", "locale"),
        ("      custom_format: '%'\n", "custom_format"),
        ("      ignored_commits_file: missing.txt\n", "ignored_commits_file"),
    ],
)
def test_invalid_runtime_configuration_is_reported(
    tmp_path: Path, options: str, message: str
) -> None:
    repository(tmp_path)

    with pytest.raises(RuntimeError, match=message):
        build(tmp_path, options)


def test_generated_blog_pages_receive_build_dates_without_git_queries(
    tmp_path: Path,
) -> None:
    docs = repository(tmp_path)
    posts = docs / "blog" / "posts"
    posts.mkdir(parents=True)
    (docs / "blog" / "index.md").write_text("# Blog\n", encoding="utf-8")
    (posts / "entry.md").write_text(
        "---\ndate: 2024-01-01\n---\n# Entry\n", encoding="utf-8"
    )
    git(tmp_path, "add", "docs")
    git(tmp_path, "commit", "-qm", "Add blog", date="2024-02-01T00:00:00Z")

    build(
        tmp_path,
        "  - blog:\n      categories: false\n      pagination: false\n",
    )

    archive = tmp_path / "site/blog/archive/2024/index.html"
    html = BeautifulSoup(archive.read_text(encoding="utf-8"), "html.parser")
    assert value(html)
    assert value(html) == value(html, "creation")
    assert value(html, suffix="-hash") == ""
    assert value(html, "site_revision") == "2024-02-01 00:00:00"


def test_missing_git_and_disabled_plugin(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repository(tmp_path)
    monkeypatch.setenv("PATH", str(tmp_path / "missing-executables"))

    with pytest.raises(RuntimeError, match="unable to execute git"):
        build(tmp_path)

    html = build(tmp_path, "      fallback_to_build_date: true\n")

    assert value(html) == value(html, "creation")
    assert value(html, suffix="-hash") == ""

    # Git and presentation settings are unused when the plugin is disabled.
    html = build(
        tmp_path, "      enabled: false\n      timezone: No/Such_Zone\n"
    )

    assert value(html) == ""
    assert "{{ git_revision_date_localized }}" in html.get_text()


def test_serve_refreshes_unchanged_pages_on_the_next_content_rebuild(
    tmp_path: Path,
) -> None:
    docs = repository(tmp_path)
    build(tmp_path)
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        config.read_text(encoding="utf-8") + "dev_addr: 127.0.0.1:0\n",
        encoding="utf-8",
    )
    output = tmp_path / "site" / "index.html"
    log_path = tmp_path / "serve.log"
    log = log_path.open("w", encoding="utf-8")
    process = subprocess.Popen(
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

    def wait_for(expected: str, *, ready: bool = False) -> None:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            if output.is_file():
                html = BeautifulSoup(
                    output.read_text(encoding="utf-8"), "html.parser"
                )
                if (
                    html.select_one("data#git_revision_date_localized")
                    and value(html) == expected
                    and (
                        not ready
                        or "Serving " in log_path.read_text(encoding="utf-8")
                    )
                ):
                    return
            time.sleep(0.02)
        raise AssertionError(log_path.read_text(encoding="utf-8"))

    try:
        wait_for("2024-01-01 00:00:00", ready=True)

        # The committed file is left unchanged,
        # as after committing during serve.
        git(
            tmp_path,
            "commit",
            "--amend",
            "--no-edit",
            "--reset-author",
            "-q",
            date="2024-05-01T00:00:00Z",
        )
        (docs / "trigger.md").write_text(
            "# A content rebuild\n", encoding="utf-8"
        )

        wait_for("2024-05-01 00:00:00")

        # A different branch with the same working file
        # also refreshes retained pages.
        git(tmp_path, "checkout", "-q", "--detach", "HEAD@{1}")
        (docs / "trigger.md").write_text(
            "# Another rebuild\n", encoding="utf-8"
        )

        wait_for("2024-01-01 00:00:00")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        log.close()
