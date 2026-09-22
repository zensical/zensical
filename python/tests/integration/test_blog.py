# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

"""Integration tests for native Material blog compatibility."""

from __future__ import annotations

import json
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


def _project(
    root: Path,
    *,
    per_page: int = 10,
    archive: bool = False,
    categories: bool = False,
    authors: bool = False,
    author_profiles: bool = False,
    entrypoint: bool = True,
) -> Path:
    docs = root / "docs"
    posts = docs / "blog" / "posts"
    overrides = root / "overrides"
    posts.mkdir(parents=True)
    overrides.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    if entrypoint:
        (docs / "blog" / "index.md").write_text(
            "# Journal\n", encoding="utf-8"
        )
    (overrides / "main.html").write_text(
        "{{ page.title }}|{{ page.url }}|{{ page.content }}",
        encoding="utf-8",
    )
    (overrides / "blog.html").write_text(
        "BLOG|{{ page.title }}|{{ page.url }}|{{ page.content }}|"
        "{% for post in posts %}{{ post.title }}:{{ post.content }}"
        "{% for category in post.categories %}#{{ category.title }}@"
        "{{ category.url }}{% endfor %}"
        "{% for author in post.authors %}@{{ author.name }}:"
        "{{ author.avatar }}:{{ author.url }}{% endfor %};{% endfor %}|"
        "{% if pagination %}{{ pagination.page }}/{{ pagination.pages }}:"
        "NEXT={{ pagination.next.url if pagination.next else '' }}"
        "{% endif %}|SELF={{ page.url | url }}|NAV|"
        "{% for item in nav.items %}{{ item.title }}("
        "{% for child in item.children %}{{ child.title }}["
        "{% for leaf in child.children %}{{ leaf.title }}="
        "{{ leaf.active }},{% endfor %}];"
        "{% endfor %});{% endfor %}|TOC|"
        "{% for section in page.toc %}{{ section.title }}["
        "{% for child in section.children %}{{ child.title }}="
        "{{ child.url }}("
        "{% for leaf in child.children %}{{ leaf.title }}={{ leaf.url }},"
        "{% endfor %});{% endfor %}];{% endfor %}",
        encoding="utf-8",
    )
    (overrides / "blog-post.html").write_text(
        "POST|{{ page.title }}|{{ page.url }}|"
        "{{ page.config.date.created | date }}|{{ page.parent.url }}|"
        "{{ page.content }}|PREV={{ page.previous_page.url }}|"
        "NEXT={{ page.next_page.url }}|READ={{ page.config.readtime }}|"
        "{% for author in page.authors %}AUTHOR={{ author.name }}:"
        "{{ author.description }}:{{ author.avatar }}:{{ author.url }}"
        "{% endfor %}|LINKS="
        "{% for link in page.config.links %}{{ link.title }}={{ link.url }}="
        "{{ link.meta.subtitle if link.meta else '' }}["
        "{% for child in link.children %}{{ child.title }}={{ child.url }};"
        "{% endfor %}];{% endfor %}",
        encoding="utf-8",
    )
    config = root / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Test
theme:
  name: material
  custom_dir: overrides
plugins:
  - material/blog:
      archive: {str(archive).lower()}
      categories: {str(categories).lower()}
      authors: {str(authors).lower()}
      authors_profiles: {str(author_profiles).lower()}
      pagination_per_page: {per_page}
""",
        encoding="utf-8",
    )
    return config


def _post(
    root: Path,
    name: str,
    title: str,
    date: str,
    *,
    body: str = "Body.",
    **meta: object,
) -> None:
    lines = ["---", f"date: {date}", f"title: {title}"]
    lines.extend(
        f"{key}: {json.dumps(value)}" for key, value in meta.items()
    )
    lines.extend(["---", f"# {title}", "", body])
    (root / "docs" / "blog" / "posts" / name).write_text(
        "\n".join(lines) + "\n",
        encoding="utf-8",
    )


def test_posts_are_routed_from_dates_and_native_unicode_slugs(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    _post(tmp_path, "hello.md", "Héllo, World!", "2026-09-03")
    _post(tmp_path, "draft.md", "Draft", "2026-09-04", draft=True)

    zensical.build(str(config), _BUILD_OPTIONS)

    output = tmp_path / "site" / "blog" / "2026" / "09" / "03"
    assert output.joinpath("héllo-world", "index.html").is_file()
    assert "|READ=1" in output.joinpath(
        "héllo-world", "index.html"
    ).read_text("utf-8")
    assert not output.parent.joinpath("04", "draft", "index.html").exists()
    assert not (tmp_path / "site" / "blog" / "posts" / "hello").exists()


def test_explicit_post_metadata_controls_route_order_and_readtime(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    _post(tmp_path, "newer.md", "Newer", "2026-09-03")
    _post(
        tmp_path,
        "pinned.md",
        "Pinned",
        "2026-09-01",
        slug="explicit-route",
        pin=True,
        readtime=17,
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "01"
        / "explicit-route"
        / "index.html"
    ).read_text("utf-8")
    assert "|READ=17|" in post
    view = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert view.index("Pinned:") < view.index("Newer:")


def test_paginated_blog_pages_are_generated_without_source_files(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, per_page=1)
    _post(tmp_path, "one.md", "One", "2026-09-01")
    _post(tmp_path, "two.md", "Two", "2026-09-02")

    zensical.build(str(config), _BUILD_OPTIONS)

    page = tmp_path / "site" / "blog" / "page" / "2" / "index.html"
    assert page.is_file()
    second = page.read_text(encoding="utf-8")
    assert "BLOG|Journal|blog/page/2/" in second
    assert "One:" in second
    assert "|2/2" in second
    assert "|SELF=../../|" in second
    first = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "|1/2:NEXT=blog/page/2/|" in first
    assert not (tmp_path / "docs" / "blog" / "page").exists()
    one = (
        tmp_path / "site" / "blog" / "2026" / "09" / "01" / "one"
    ).joinpath("index.html").read_text("utf-8")
    two = (
        tmp_path / "site" / "blog" / "2026" / "09" / "02" / "two"
    ).joinpath("index.html").read_text("utf-8")
    assert "|PREV=|NEXT=blog/2026/09/02/two/" in one
    assert "|PREV=blog/2026/09/01/one/|NEXT=" in two


def test_single_page_keeps_empty_pagination_context(tmp_path: Path) -> None:
    config = _project(tmp_path)
    _post(tmp_path, "one.md", "One", "2026-09-01")

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "|1/1:NEXT=|" in page


def test_serve_reconciles_routes_views_and_pagination(tmp_path: Path) -> None:
    """Retained blog revisions retract every superseded output."""
    config = _project(tmp_path, per_page=1, archive=True, categories=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("dev_addr: 127.0.0.1:0\n")
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        categories=["Alpha"],
    )
    _post(
        tmp_path,
        "two.md",
        "Two",
        "2026-09-02",
        categories=["Alpha"],
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
        ],
        cwd=tmp_path,
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    site = tmp_path / "site" / "blog"
    index = site / "index.html"
    second = site / "page" / "2" / "index.html"
    one = site / "2026" / "09" / "01" / "one" / "index.html"
    two = site / "2026" / "09" / "02" / "two" / "index.html"
    moved = site / "2026" / "09" / "03" / "moved" / "index.html"
    alpha = site / "category" / "alpha" / "index.html"
    beta = site / "category" / "beta" / "index.html"

    def contains(path: Path, value: str) -> bool:
        try:
            return value in path.read_text(encoding="utf-8")
        except OSError:
            return False

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
            f"serve did not reconcile blog state: {log.read()}"
        )

    try:
        wait_for(
            lambda: (
                two.is_file()
                and one.is_file()
                and contains(index, "Two:")
                and contains(second, "One:")
            )
        )
        _post(
            tmp_path,
            "one.md",
            "Moved",
            "2026-09-03",
            categories=["Beta"],
        )
        wait_for(
            lambda: (
                moved.is_file()
                and not one.exists()
                and beta.is_file()
                and contains(index, "Moved:")
                and contains(second, "Two:")
            )
        )
        (tmp_path / "docs" / "blog" / "posts" / "two.md").unlink()
        wait_for(
            lambda: (
                not two.exists()
                and not second.exists()
                and not alpha.exists()
                and contains(index, "Moved:")
            )
        )
        assert process.poll() is None
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        log.close()


def test_pagination_format_exposes_ordered_native_items(tmp_path: Path) -> None:
    config = _project(tmp_path, per_page=2)
    with config.open("a", encoding="utf-8") as stream:
        stream.write(
            "      pagination_format: >-\n"
            "        $link_first|$link_previous|${page}/$page_count|"
            "$first_item-$last_item/$item_count|$link_next|$link_last|"
            "$$|$unknown\n"
        )
    (tmp_path / "overrides" / "blog.html").write_text(
        "{% for item in pagination.items %}"
        "[{{ item.type }}:{{ item.value }}:"
        "{{ item.page if item.page else '' }}:"
        "{{ item.url if item.url else '' }}]"
        "{% endfor %}",
        encoding="utf-8",
    )
    _post(tmp_path, "one.md", "One", "2026-09-01")
    _post(tmp_path, "two.md", "Two", "2026-09-02")
    _post(tmp_path, "three.md", "Three", "2026-09-03")

    zensical.build(str(config), _BUILD_OPTIONS)

    first = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "[text:||1/2|1-2/3|::]" in first
    assert "[next_page:2:2:blog/page/2/]" in first
    assert "[last_page:2:2:blog/page/2/]" in first
    assert "[text:|$|$unknown::]" in first

    second = (
        tmp_path / "site" / "blog" / "page" / "2" / "index.html"
    ).read_text("utf-8")
    assert "[first_page:1:1:blog/]" in second
    assert "[previous_page:1:1:blog/]" in second
    assert "[text:|2/2|3-3/3|||$|$unknown::]" in second


def test_empty_blog_has_no_reachable_pagination_pages(tmp_path: Path) -> None:
    config = _project(tmp_path)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("      pagination_if_single_page: true\n")
    (tmp_path / "overrides" / "blog.html").write_text(
        "{{ pagination.page }}/{{ pagination.pages }}|"
        "{{ pagination.items | length }}|"
        "{{ pagination.first_page if pagination.first_page else 'none' }}|"
        "{{ pagination.first_item if pagination.first_item else 'none' }}",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert page == "1/0|0|none|none"


def test_empty_blog_omits_unreachable_archive_navigation(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, archive=True)

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "Archive[" not in page
    assert not (tmp_path / "site" / "blog" / "archive").exists()


def test_missing_blog_entrypoint_is_generated_without_mutating_docs(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, entrypoint=False)

    zensical.build(str(config), _BUILD_OPTIONS)

    page = tmp_path / "site" / "blog" / "index.html"
    assert page.is_file()
    assert "BLOG|Blog|blog/" in page.read_text("utf-8")
    assert "Blog(" in page.read_text("utf-8")
    assert not (tmp_path / "docs" / "blog" / "index.md").exists()


def test_archive_and_category_views_share_native_pagination_pipeline(
    tmp_path: Path,
) -> None:
    config = _project(
        tmp_path,
        per_page=1,
        archive=True,
        categories=True,
    )
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        categories=["Rust"],
    )
    _post(
        tmp_path,
        "two.md",
        "Two",
        "2026-09-02",
        categories=["Rust"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    archive = tmp_path / "site" / "blog" / "archive" / "2026"
    category = tmp_path / "site" / "blog" / "category" / "rust"
    assert "Two:" in archive.joinpath("index.html").read_text("utf-8")
    assert "One:" in archive.joinpath("page", "2", "index.html").read_text(
        "utf-8"
    )
    assert "Two:" in category.joinpath("index.html").read_text("utf-8")
    assert "One:" in category.joinpath("page", "2", "index.html").read_text(
        "utf-8"
    )
    navigation = archive.joinpath("index.html").read_text("utf-8")
    assert "Archive[2026=" in navigation
    assert "Categories[Rust=" in navigation
    assert "#Rust@blog/category/rust/" in navigation
    assert "Posts" not in navigation
    paginated = category.joinpath("page", "2", "index.html").read_text(
        "utf-8"
    )
    assert "Categories[Rust=true,]" in paginated
    assert not (tmp_path / "docs" / "blog" / "archive").exists()
    assert not (tmp_path / "docs" / "blog" / "category").exists()


def test_navigation_labels_use_theme_language_partial(tmp_path: Path) -> None:
    config = _project(
        tmp_path,
        archive=True,
        categories=True,
        authors=True,
        author_profiles=True,
    )
    text = config.read_text(encoding="utf-8")
    config.write_text(
        text.replace(
            "  custom_dir: overrides\n",
            "  custom_dir: overrides\n  language: de\n",
        ),
        encoding="utf-8",
    )
    (tmp_path / "docs" / "blog" / ".authors.yml").write_text(
        "authors:\n"
        "  jane:\n"
        "    name: Jane Doe\n"
        "    description: Technical writer\n"
        "    avatar: assets/jane.png\n",
        encoding="utf-8",
    )
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        categories=["Rust"],
        authors=["jane"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "Archiv[2026=" in page
    assert "Kategorien[Rust=" in page
    assert "Autoren[Jane Doe=" in page


def test_navigation_labels_keep_literal_configuration(tmp_path: Path) -> None:
    config = _project(tmp_path, archive=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("      archive_name: History\n")
    _post(tmp_path, "one.md", "One", "2026-09-01")

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "History[2026=" in page


def test_archive_navigation_follows_post_order_for_nonnumeric_urls(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, archive=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write(
            '      archive_date_format: "MMMM yyyy"\n'
            "      archive_url_date_format: MMMM\n"
        )
    _post(tmp_path, "november.md", "November", "2026-11-01")
    _post(tmp_path, "december.md", "December", "2026-12-01")

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert page.index("December 2026=") < page.index("November 2026=")


def test_archive_url_keys_can_group_multiple_display_dates(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, archive=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write(
            "      pagination: false\n"
            '      archive_url_date_format: "\'all\'"\n'
        )
    _post(tmp_path, "older.md", "Older", "2025-01-01")
    _post(tmp_path, "newer.md", "Newer", "2026-01-01")

    zensical.build(str(config), _BUILD_OPTIONS)

    archive = tmp_path / "site" / "blog" / "archive" / "all" / "index.html"
    page = archive.read_text("utf-8")
    assert "BLOG|2026|" in page
    assert "Newer:" in page
    assert "Older:" in page
    assert not (tmp_path / "site" / "blog" / "archive" / "2025").exists()


def test_authored_category_page_becomes_the_view_source(tmp_path: Path) -> None:
    config = _project(tmp_path, per_page=1, categories=True)
    category = tmp_path / "docs" / "blog" / "category"
    category.mkdir()
    category.joinpath("rust.md").write_text(
        "---\ntitle: Custom Rust\n---\n# Authored category\n",
        encoding="utf-8",
    )
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        categories=["Rust"],
    )
    _post(
        tmp_path,
        "two.md",
        "Two",
        "2026-09-02",
        categories=["Rust"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    first = tmp_path / "site" / "blog" / "category" / "rust"
    first_html = first.joinpath("index.html").read_text("utf-8")
    second_html = first.joinpath("page", "2", "index.html").read_text("utf-8")
    assert "BLOG|Custom Rust|" in first_html
    assert "Authored category" in first_html
    assert "BLOG|Custom Rust|" in second_html
    assert first_html.count("Categories[") == 1


def test_excerpt_links_are_rebased_for_each_containing_view(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, per_page=1)
    (tmp_path / "docs" / "notes.md").write_text("# Notes\n", encoding="utf-8")
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        body="[Notes](../../notes.md)\n\n<!-- more -->\n\nRemainder.",
    )
    _post(tmp_path, "two.md", "Two", "2026-09-02")

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (
        tmp_path / "site" / "blog" / "page" / "2" / "index.html"
    ).read_text("utf-8")
    assert 'href="../../../notes/"' in page
    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "01"
        / "one"
        / "index.html"
    ).read_text("utf-8")
    assert 'href="../../../../../notes/"' in post


def test_required_excerpt_separator_is_validated(tmp_path: Path) -> None:
    config = _project(tmp_path)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("      post_excerpt: required\n")
    _post(tmp_path, "one.md", "One", "2026-09-01")

    with pytest.raises(RuntimeError) as error:
        zensical.build(str(config), _BUILD_OPTIONS)
    assert "requires the excerpt separator '<!-- more -->'" in str(error.value)


def test_url_formats_do_not_require_a_slug_placeholder(tmp_path: Path) -> None:
    config = _project(tmp_path)
    with config.open("a", encoding="utf-8") as stream:
        stream.write('      post_url_format: "{date}/{file}"\n')
    _post(tmp_path, "source-name.md", "Different title", "2026-09-01")

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "01"
        / "source-name"
        / "index.html"
    ).is_file()


def test_disabling_pagination_keeps_all_posts_on_one_view(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, per_page=1)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("      pagination: false\n")
    _post(tmp_path, "one.md", "One", "2026-09-01")
    _post(tmp_path, "two.md", "Two", "2026-09-02")

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "One:" in page
    assert "Two:" in page
    assert not (tmp_path / "site" / "blog" / "page").exists()


def test_multiple_blog_instances_keep_routes_and_views_isolated(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    for directory, title in [("journal", "Journal"), ("news", "News")]:
        root = tmp_path / "docs" / directory
        root.joinpath("posts").mkdir(parents=True)
        root.joinpath("index.md").write_text(f"# {title}\n", encoding="utf-8")
        root.joinpath("posts", "entry.md").write_text(
            f"---\ndate: 2026-09-01\n---\n# {title} entry\n",
            encoding="utf-8",
        )
    config.write_text(
        """\
site_name: Test
theme:
  name: material
  custom_dir: overrides
plugins:
  - material/blog:
      blog_dir: journal
      post_dir: journal/posts
      archive: false
      categories: false
      authors: false
  - material/blog:
      blog_dir: news
      post_dir: news/posts
      archive: false
      categories: false
      authors: false
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (
        tmp_path
        / "site"
        / "journal"
        / "2026"
        / "09"
        / "01"
        / "journal-entry"
        / "index.html"
    ).is_file()
    assert (
        tmp_path
        / "site"
        / "news"
        / "2026"
        / "09"
        / "01"
        / "news-entry"
        / "index.html"
    ).is_file()


def test_categories_can_sort_by_post_count(tmp_path: Path) -> None:
    config = _project(tmp_path, categories=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write(
            "      categories_sort_by:\n"
            "        object: material.plugins.blog.view_post_count\n"
            "      categories_sort_reverse: true\n"
        )
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        categories=["Alpha", "Beta"],
    )
    _post(
        tmp_path,
        "two.md",
        "Two",
        "2026-09-02",
        categories=["Beta"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert page.index("Categories[Beta=") < page.index("Alpha=")


def test_authors_are_resolved_and_profiles_use_native_routes(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, authors=True, author_profiles=True)
    (tmp_path / "docs" / "blog" / ".authors.yml").write_text(
        """\
authors:
  jane:
    name: Jane Doe
    description: Technical writer
    avatar: assets/jane.png
    slug: jane-doe
""",
        encoding="utf-8",
    )
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        authors=["jane"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "01"
        / "one"
        / "index.html"
    ).read_text("utf-8")
    assert (
        "AUTHOR=Jane Doe:Technical writer:assets/jane.png:"
        "blog/author/jane-doe/"
    ) in post
    profile = (
        tmp_path / "site" / "blog" / "author" / "jane-doe" / "index.html"
    )
    assert profile.is_file()
    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "@Jane Doe:assets/jane.png:blog/author/jane-doe/" in page
    assert "Authors[Jane Doe=" in page
    assert not (tmp_path / "site" / "blog" / ".authors.yml").exists()


def test_author_profiles_follow_first_appearance_in_post_order(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, authors=True, author_profiles=True)
    (tmp_path / "docs" / "blog" / ".authors.yml").write_text(
        """\
authors:
  alpha:
    name: Alpha Author
    description: Alpha
    avatar: alpha.png
  zeta:
    name: Zeta Author
    description: Zeta
    avatar: zeta.png
""",
        encoding="utf-8",
    )
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        authors=["zeta", "alpha"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert page.index("Zeta Author=") < page.index("Alpha Author=")


def test_unknown_post_author_is_rejected(tmp_path: Path) -> None:
    config = _project(tmp_path, authors=True)
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        authors=["missing"],
    )

    with pytest.raises(RuntimeError, match="couldn't find author 'missing'"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_custom_author_catalog_is_consumed_without_being_published(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, authors=True, author_profiles=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("      authors_file: '{blog}/people.yml'\n")
    (tmp_path / "docs" / "blog" / "people.yml").write_text(
        """\
authors:
  jane:
    name: Jane Doe
    description: Technical writer
    avatar: jane.png
    slug: jane-doe
""",
        encoding="utf-8",
    )
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        authors=["jane"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    profile = (
        tmp_path / "site" / "blog" / "author" / "jane-doe" / "index.html"
    )
    assert profile.is_file()
    assert not (tmp_path / "site" / "blog" / "people.yml").exists()


def test_post_assets_are_relocated_to_the_public_blog_tree(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    assets = tmp_path / "docs" / "blog" / "posts" / "assets"
    assets.mkdir()
    assets.joinpath("image.png").write_bytes(b"image")
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        body="![Image](assets/image.png)",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (tmp_path / "site" / "blog" / "assets" / "image.png").is_file()
    assert not (
        tmp_path / "site" / "blog" / "posts" / "assets" / "image.png"
    ).exists()
    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "01"
        / "one"
        / "index.html"
    ).read_text("utf-8")
    assert 'src="../../../../assets/image.png"' in post
    view = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert 'src="assets/image.png"' in view
    assert "posts/assets/image.png" not in post
    assert "posts/assets/image.png" not in view


def test_nested_post_asset_links_preserve_suffixes_and_url_boundaries(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    posts = tmp_path / "docs" / "blog" / "posts"
    media = posts / "nested" / "media"
    media.mkdir(parents=True)
    media.joinpath("image.svg").write_text("<svg></svg>", encoding="utf-8")
    media.joinpath("reference.txt").write_text("reference", encoding="utf-8")
    _post(
        tmp_path,
        "nested/one.md",
        "One",
        "2026-09-01",
        body=(
            "[Markdown](media/reference.txt?download=1#part)\n\n"
            "![Image](media/image.svg?version=2#icon)\n\n"
            '<a href="media/reference.txt?raw=1#part">Raw</a>\n'
            '<img src="media/image.svg?raw=1#icon" alt="Raw image">\n\n'
            "[Root](/shared.txt) [External](https://example.org/file)"
        ),
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = tmp_path / "site" / "blog" / "nested" / "media"
    assert output.joinpath("image.svg").is_file()
    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "01"
        / "one"
        / "index.html"
    ).read_text("utf-8")
    assert "../../../../nested/media/reference.txt?download=1#part" in post
    assert "../../../../nested/media/image.svg?version=2#icon" in post
    assert "../../../../nested/media/reference.txt?raw=1#part" in post
    assert "../../../../nested/media/image.svg?raw=1#icon" in post
    assert 'href="/shared.txt"' in post
    assert 'href="https://example.org/file"' in post
    view = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "nested/media/reference.txt?download=1#part" in view
    assert "nested/media/reference.txt?raw=1#part" in view


@pytest.mark.parametrize(
    ("date", "categories", "message"),
    [
        ("not-a-date", None, "invalid date"),
        ("2026-09-01", ["Forbidden"], "outside categories_allowed"),
    ],
)
def test_invalid_post_metadata_is_rejected(
    tmp_path: Path,
    date: str,
    categories: list[str] | None,
    message: str,
) -> None:
    config = _project(tmp_path, categories=categories is not None)
    if categories is not None:
        with config.open("a", encoding="utf-8") as stream:
            stream.write("      categories_allowed: [Allowed]\n")
    if categories is None:
        _post(tmp_path, "one.md", "One", date)
    else:
        _post(
            tmp_path,
            "one.md",
            "One",
            date,
            categories=categories,
        )

    with pytest.raises(RuntimeError, match=message):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_date_display_formats_are_independent_from_archive_routes(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, archive=True)
    with config.open("a", encoding="utf-8") as stream:
        stream.write(
            "      post_date_format: medium\n"
            '      archive_date_format: "MMMM yyyy"\n'
        )
    _post(tmp_path, "one.md", "One", "2026-09-03")

    zensical.build(str(config), _BUILD_OPTIONS)

    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "03"
        / "one"
        / "index.html"
    ).read_text("utf-8")
    assert "|Sep 3, 2026|" in post
    archive = tmp_path / "site" / "blog" / "archive" / "2026"
    assert archive.joinpath("index.html").is_file()
    assert "Archive[September 2026=" in archive.joinpath(
        "index.html"
    ).read_text("utf-8")


def test_fractional_post_dates_preserve_values_and_order(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    _post(
        tmp_path,
        "a-older.md",
        "Older",
        "2026-09-03T10:30:00.100000Z",
    )
    _post(
        tmp_path,
        "z-newer.md",
        "Newer",
        "2026-09-03T10:30:00.200000Z",
    )
    (tmp_path / "overrides" / "blog-post.html").write_text(
        "{{ page.config.date.created }}",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    view = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert view.index("Newer:") < view.index("Older:")
    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "03"
        / "newer"
        / "index.html"
    ).read_text("utf-8")
    assert post == "2026-09-03 10:30:00.200000+00:00"


def test_structured_links_resolve_pages_anchors_and_nested_sections(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    (tmp_path / "docs" / "guide.md").write_text(
        "# Guide\n\n## Details\n",
        encoding="utf-8",
    )
    assets = tmp_path / "docs" / "blog" / "posts" / "assets"
    assets.mkdir()
    assets.joinpath("reference.pdf").write_bytes(b"reference")
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        links=[
            {"Guide section": "guide.md#details"},
            {"Download": "blog/posts/assets/reference.pdf"},
            {"Resources": [{"External": "https://example.com"}]},
        ],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    post = (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "09"
        / "01"
        / "one"
        / "index.html"
    ).read_text("utf-8")
    assert "Guide section=guide/#details=Details[]" in post
    assert "Download=blog/assets/reference.pdf=" in post
    assert "Resources=none=[External=https://example.com;]" in post


def test_excerpt_toc_contains_only_the_post_root(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("      blog_toc: true\n")
    _post(
        tmp_path,
        "one.md",
        "One",
        "2026-09-01",
        body=(
            "## Included\n\n[Jump](#included)\n\n"
            "<!-- more -->\n\n"
            "## Excluded\n\nMore."
        ),
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert "TOC|Journal[One=2026/09/01/one/();" in page
    assert "Included=" not in page
    assert "Excluded=" not in page
    assert (
        '<h3 id="included"><a class="toclink" '
        'href="2026/09/01/one/#included">Included</a></h3>'
        in page
    )
    assert '<a href="2026/09/01/one/#included">Jump</a>' in page


def test_excerpt_inserts_a_linked_title_when_the_post_has_no_h1(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    (tmp_path / "docs" / "blog" / "posts" / "one.md").write_text(
        "---\ndate: 2026-09-01\ntitle: One & Two\n---\nBody only.\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    page = (tmp_path / "site" / "blog" / "index.html").read_text("utf-8")
    assert (
        '<h2 id="one-two"><a class="toclink" '
        'href="2026/09/01/one--two/">One &amp; Two</a></h2>'
        in page
    )


def test_standalone_blog_uses_root_entrypoint_and_post_routes(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    posts = tmp_path / "docs" / "posts"
    posts.mkdir()
    posts.joinpath("one.md").write_text(
        "---\ndate: 2026-09-01\n---\n# One\n",
        encoding="utf-8",
    )
    config.write_text(
        """\
site_name: Test
theme:
  name: material
  custom_dir: overrides
plugins:
  - material/blog:
      blog_dir: .
      archive: false
      categories: false
      authors: false
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (tmp_path / "site" / "index.html").is_file()
    assert (
        tmp_path
        / "site"
        / "2026"
        / "09"
        / "01"
        / "one"
        / "index.html"
    ).is_file()
    assert not (tmp_path / "site" / "posts" / "one" / "index.html").exists()


def test_blog_routes_respect_disabled_directory_urls(tmp_path: Path) -> None:
    config = _project(tmp_path, per_page=1)
    with config.open("a", encoding="utf-8") as stream:
        stream.write("use_directory_urls: false\n")
    _post(tmp_path, "one.md", "One", "2026-09-01")
    _post(tmp_path, "two.md", "Two", "2026-09-02")

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (
        tmp_path / "site" / "blog" / "2026" / "09" / "01" / "one.html"
    ).is_file()
    assert (
        tmp_path / "site" / "blog" / "page" / "2" / "index.html"
    ).is_file()


def test_posts_receive_inherited_meta_before_blog_classification(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, categories=True)
    config.write_text(
        config.read_text(encoding="utf-8").replace(
            "plugins:\n",
            "plugins:\n  - material/meta\n",
        ),
        encoding="utf-8",
    )
    posts = tmp_path / "docs" / "blog" / "posts"
    (posts / ".meta.yml").write_text(
        "date: 2026-08-31\ncategories: [Inherited]\n",
        encoding="utf-8",
    )
    (posts / "inherited.md").write_text(
        "---\ntitle: Inherited post\n---\n# Content heading\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (
        tmp_path
        / "site"
        / "blog"
        / "2026"
        / "08"
        / "31"
        / "inherited-post"
        / "index.html"
    ).is_file()
    category = (
        tmp_path / "site" / "blog" / "category" / "inherited" / "index.html"
    ).read_text("utf-8")
    assert "Inherited post:" in category


def test_search_uses_final_blog_post_routes(tmp_path: Path) -> None:
    config = _project(tmp_path)
    config.write_text(
        config.read_text(encoding="utf-8").replace(
            "plugins:\n",
            "plugins:\n  - search\n",
        ),
        encoding="utf-8",
    )
    _post(tmp_path, "one.md", "Searchable", "2026-09-01")

    zensical.build(str(config), _BUILD_OPTIONS)

    index = json.loads((tmp_path / "site" / "search.json").read_text("utf-8"))
    locations = [item["location"] for item in index["items"]]
    assert "blog/2026/09/01/searchable/" in locations


def test_tag_listings_link_to_final_blog_post_routes(tmp_path: Path) -> None:
    config = _project(tmp_path)
    config.write_text(
        config.read_text(encoding="utf-8").replace(
            "plugins:\n",
            "plugins:\n  - material/tags\n",
        ),
        encoding="utf-8",
    )
    (tmp_path / "docs" / "tags.md").write_text(
        "# Tags\n\n<!-- material/tags -->\n",
        encoding="utf-8",
    )
    _post(
        tmp_path,
        "one.md",
        "Tagged post",
        "2026-09-01",
        tags=["Feature"],
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "tags" / "index.html").read_text("utf-8")
    assert 'href="../blog/2026/09/01/tagged-post/"' in listing
