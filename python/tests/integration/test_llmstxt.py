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

"""LLM text artifacts from the rendered page pipeline."""

from __future__ import annotations

import subprocess
import sys
import time
from textwrap import dedent
from typing import TYPE_CHECKING

import pytest
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


def build(root: Path, options: str, *, strict: bool = False) -> Path:
    """Build the supplied project while retaining caches between builds."""
    config = root / "mkdocs.yml"
    config.write_text(
        dedent("""\
            site_name: LLM test
            site_url: https://example.org/docs/
        """)
        + options,
        encoding="utf-8",
    )
    zensical.build(str(config), {"clean": False, "strict": strict})
    return root / "site"


@pytest.mark.parametrize("directory_urls", [True, False])
@pytest.mark.parametrize("enabled", [True, False])
def test_exposes_markdown_urls_only_for_exported_pages(
    tmp_path: Path, directory_urls: bool, enabled: bool
) -> None:
    # An encoded filename and a different export host
    # are used under a site subpath.
    docs = tmp_path / "docs"
    (docs / "guide").mkdir(parents=True)
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    (docs / "guide" / "a # café.md").write_text("# Guide\n", encoding="utf-8")
    (docs / "excluded.md").write_text("# Excluded\n", encoding="utf-8")
    overrides = tmp_path / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(
        '{% if page.markdown_url %}<a href="{{ page.markdown_url | url }}" '
        'data-url="{{ page.markdown_url }}">Markdown</a>{% endif %}',
        encoding="utf-8",
    )
    config = dedent(f"""\
        use_directory_urls: {str(directory_urls).lower()}
        theme:
          custom_dir: overrides
        plugins:
          - llmstxt:
              enabled: {str(enabled).lower()}
              base_url: https://cdn.example.org/exports/
              sections: {{Pages: [index.md, "guide/*.md"]}}
    """)
    guide = (
        "guide/a # café/index.html" if directory_urls else "guide/a # café.html"
    )
    excluded = "excluded/index.html" if directory_urls else "excluded.html"
    guide_url = (
        "guide/a%20%23%20caf%C3%A9/index.md"
        if directory_urls
        else "guide/a%20%23%20caf%C3%A9.md"
    )
    guide_href = "./index.md" if directory_urls else "./a%20%23%20caf%C3%A9.md"

    # The same URLs must be exposed on a fresh build and a cached build.
    for _ in range(2):
        site = build(tmp_path, config)

        for output, url, href in (
            ("index.html", "index.md", "./index.md"),
            (guide, guide_url, guide_href),
        ):
            page = BeautifulSoup(
                (site / output).read_text(encoding="utf-8"), "html.parser"
            )
            link = page.find("a")
            if enabled:
                assert link is not None
                assert link["data-url"] == url
                assert link["href"] == href
                assert (site / output).with_suffix(".md").is_file()
            else:
                assert link is None

        assert "<a " not in (site / excluded).read_text(encoding="utf-8")


def test_markdown_url_follows_the_published_blog_route(tmp_path: Path) -> None:
    # The post is published at a date-based route instead of its source path.
    docs = tmp_path / "docs"
    posts = docs / "blog" / "posts"
    posts.mkdir(parents=True)
    (docs / "blog" / "index.md").write_text("# Blog\n", encoding="utf-8")
    (posts / "entry.md").write_text(
        "---\ndate: 2026-09-03\n---\n# Hello\n", encoding="utf-8"
    )
    overrides = tmp_path / "overrides"
    overrides.mkdir()
    (overrides / "blog-post.html").write_text(
        "{{ page.markdown_url }}", encoding="utf-8"
    )

    site = build(
        tmp_path,
        dedent("""\
        theme:
          custom_dir: overrides
        plugins:
          - material/blog:
              archive: false
              categories: false
          - llmstxt:
              sections: {Posts: ["blog/posts/*.md"]}
    """),
    )

    post = site / "blog/2026/09/03/hello/index.html"
    assert post.read_text(encoding="utf-8") == "blog/2026/09/03/hello/index.md"
    assert post.with_suffix(".md").read_text(encoding="utf-8") == "# Hello\n"


@pytest.mark.parametrize("directory_urls", [True, False])
def test_emits_selected_pages_in_section_order_and_uses_final_titles(
    tmp_path: Path, directory_urls: bool
) -> None:
    docs = tmp_path / "docs"
    (docs / "guide" / "nested").mkdir(parents=True)
    (docs / "index.md").write_text("# Home\n\nWelcome.\n", encoding="utf-8")
    (docs / "guide" / "a.md").write_text("# First\n\nOne.\n", encoding="utf-8")
    (docs / "guide" / "nested" / "b.md").write_text(
        "# Second\n\nTwo.\n", encoding="utf-8"
    )
    (docs / "excluded.md").write_text("# Excluded\n", encoding="utf-8")
    config = dedent(f"""\
        use_directory_urls: {str(directory_urls).lower()}
        site_description: A short description.
        nav:
          - Navigation title: index.md
          - guide/a.md
          - guide/nested/b.md
        plugins:
          - llmstxt:
              base_url: https://cdn.example.org/v2
              markdown_description: Long **description**.
              full_output: ai/full.txt
              sections:
                Z first:
                  - index.md: Home page
                  - guide/*.md: Guide page
                  - guide/a.md: Updated description
                A second:
                  - guide/a.md
    """)

    site = build(tmp_path, config)

    first = "guide/a/index.md" if directory_urls else "guide/a.md"
    second = (
        "guide/nested/b/index.md" if directory_urls else "guide/nested/b.md"
    )
    index = (site / "llms.txt").read_text(encoding="utf-8")
    assert index == dedent(f"""\
        # LLM test

        > A short description.

        Long **description**.

        ## Z first

        - [Navigation title](<https://cdn.example.org/v2/index.md>): Home page
        - [First](<https://cdn.example.org/v2/{first}>): Updated description
        - [Second](<https://cdn.example.org/v2/{second}>): Guide page

        ## A second

        - [First](<https://cdn.example.org/v2/{first}>)
    """)
    assert (site / first).read_text(encoding="utf-8") == "# First\n\nOne.\n"
    assert (site / second).read_text(encoding="utf-8") == "# Second\n\nTwo.\n"
    assert not (site / "excluded" / "index.md").exists()
    assert not (site / "excluded.md").exists()
    full = (site / "ai" / "full.txt").read_text(encoding="utf-8")
    assert full.count("# First\n") == 2
    assert full.index("# Z first") < full.index("# A second")

    # A cached build must still emit the page Markdown and both aggregate files.
    (site / first).unlink()
    (site / "llms.txt").unlink()
    (site / "ai" / "full.txt").unlink()
    build(tmp_path, config)

    assert (site / first).read_text(encoding="utf-8") == "# First\n\nOne.\n"
    assert (site / "llms.txt").read_text(encoding="utf-8") == index
    assert (site / "ai" / "full.txt").read_text(encoding="utf-8") == full


def test_rebuilds_gfm_from_generated_html_with_search_disabled(
    tmp_path: Path,
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        dedent("""\
            # {{ config.site_name }}

            !!! note "A note"

                Generated **content**.

            ```python
            value = "```"
            ```

            | Name | Value |
            | ---- | ----- |
            | one  | two   |

            - Parent
                - Child

            - [x] Done
            - [ ] Todo

            <del>Old content</del> with a note.[^example]

            [^example]: A **formatted** footnote.
        """),
        encoding="utf-8",
    )

    site = build(
        tmp_path,
        dedent("""\
            plugins:
              - search: {enabled: false}
              - macros
              - llmstxt:
                  sections: {Pages: [index.md]}
        """),
    )

    markdown = (site / "index.md").read_text(encoding="utf-8")
    assert markdown.startswith("# LLM test\n")
    assert (
        dedent("""\
        > [!NOTE]
        >
        > **A note**
        >
        > Generated **content**.
    """)
        in markdown
    )
    assert '````python\nvalue = "```"\n````' in markdown
    assert (
        dedent("""\
        | Name | Value |
        | --- | --- |
        | one | two |
    """)
        in markdown
    )
    assert "- Parent\n\n  - Child" in markdown
    assert "- [x] Done\n- [ ] Todo" in markdown
    assert "~~Old content~~ with a note.[^/docs/#example]\n" in markdown
    assert "[^/docs/#example]: A **formatted** footnote." in markdown
    assert "footnote-backref" not in markdown
    assert "headerlink" not in markdown
    assert "{{" not in markdown
    assert "!!!" not in markdown
    assert not (site / "llms-full.txt").exists()


@pytest.mark.parametrize("autoclean", [True, False])
def test_converts_admonitions_details_and_callouts_to_github_alerts(
    tmp_path: Path, autoclean: bool
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        dedent("""\
            # Alerts

            !!! warning "Before you start"

                Save your **work**.

            ??? tip "Try `fast=True`"

                Use the faster mode.

            ???+ danger

                This deletes data.

            !!! important

                Read the instructions.

            !!! caution ""

                Handle with care.

            !!! custom "Project policy"

                Follow these rules.

            > [!NOTE] From a callout
            > The body remains readable.
        """),
        encoding="utf-8",
    )
    config = dedent(f"""\
        plugins:
          - callouts
          - llmstxt:
              autoclean: {str(autoclean).lower()}
              sections:
                Pages: [index.md]
    """)

    site = build(tmp_path, config)

    markdown = (site / "index.md").read_text(encoding="utf-8")
    assert (
        dedent("""\
        > [!WARNING]
        >
        > **Before you start**
        >
        > Save your **work**.

        > [!TIP]
        >
        > **Try `fast=True`**
        >
        > Use the faster mode.

        > [!CAUTION]
        >
        > **Danger**
        >
        > This deletes data.

        > [!IMPORTANT]
        >
        > Read the instructions.

        > [!CAUTION]
        >
        > Handle with care.

        > [!NOTE]
        >
        > **Project policy**
        >
        > Follow these rules.

        > [!NOTE]
        >
        > **From a callout**
        >
        > The body remains readable.
    """)
        in markdown
    )

    # The HTML site keeps its original admonitions and disclosure controls.
    html = (site / "index.html").read_text(encoding="utf-8")
    assert '<div class="admonition warning">' in html
    assert '<details class="tip">' in html
    assert '<details class="danger" open="open">' in html


@pytest.mark.parametrize("autoclean", [True, False])
def test_omits_mkdocstrings_source_and_empty_icons_from_llmstxt(
    tmp_path: Path, autoclean: bool
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    source = tmp_path / "src"
    source.mkdir()
    (source / "sample_api.py").write_text(
        dedent('''\
            def greet(name: str) -> str:
                """Return a greeting.

                Examples:
                    ```python
                    greet("reader")
                    ```
                """
                return f"Hello, {name}!"
        '''),
        encoding="utf-8",
    )
    (docs / "index.md").write_text(
        dedent("""\
            # API

            ::: sample_api.greet

            ## Example

            ```python
            from sample_api import greet
            print(greet("reader"))
            ```
        """),
        encoding="utf-8",
    )
    config = dedent(f"""\
        plugins:
          - mkdocstrings:
              handlers:
                python:
                  paths: [src]
                  options:
                    docstring_style: google
                    heading_level: 2
                    show_root_heading: true
                    show_root_full_path: false
                    show_symbol_type_heading: true
                    separate_signature: true
                    show_signature_annotations: true
                    show_source: true
          - llmstxt:
              autoclean: {str(autoclean).lower()}
              full_output: llms-full.txt
              sections:
                Pages: [index.md]
    """)

    site = build(tmp_path, config)

    # Both LLM outputs keep the API documentation and examples
    # without the implementation (source code).
    for name in ("index.md", "llms-full.txt"):
        markdown = (site / name).read_text(encoding="utf-8")
        assert "## greet" in markdown
        assert markdown.count("Return a greeting.") == 1
        assert "greet(name: str) -> str" in markdown
        assert (
            dedent("""\
            ```python
            greet("reader")
            ```
        """)
            in markdown
        )
        assert (
            dedent("""\
            ```python
            from sample_api import greet
            print(greet("reader"))
            ```
        """)
            in markdown
        )
        assert "<code></code>" not in markdown
        assert "Source code in" not in markdown
        assert "Hello," not in markdown

    # The website still includes the source listing and heading icon.
    html = (site / "index.html").read_text(encoding="utf-8")
    assert 'class="mkdocstrings-source"' in html
    assert (
        'class="doc-symbol doc-symbol-heading doc-symbol-function"></code>'
        in html
    )
    assert "Source code in" in html
    assert "Hello," in html


def test_footnotes_remain_distinct_in_aggregated_output(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    for name, title in (("index", "Home"), ("second", "Second")):
        (docs / f"{name}.md").write_text(
            dedent(f"""\
                # {title}

                A note.[^same]

                [^same]: Footnote from {title}.
            """),
            encoding="utf-8",
        )

    site = build(
        tmp_path,
        dedent("""\
            plugins:
              - llmstxt:
                  full_output: llms-full.txt
                  sections: {Pages: [index.md, second.md]}
        """),
    )

    full = (site / "llms-full.txt").read_text(encoding="utf-8")

    # Each reference must still point to its page's definition
    # after aggregation.
    for path, title in (("/docs/", "Home"), ("/docs/second/", "Second")):
        assert f"A note.[^{path}#same]\n" in full
        assert f"[^{path}#same]: Footnote from {title}." in full


def test_missing_explicit_pages_warn_and_strict_builds_fail(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    config = dedent("""\
        plugins:
          - llmstxt:
              sections: {Pages: [missing.md]}
    """)

    site = build(tmp_path, config)

    assert "Page URI 'missing.md' not found" in capfd.readouterr().err
    assert (site / "llms.txt").is_file()
    assert not (site / "index.md").exists()

    with pytest.raises(Exception, match="strict"):
        build(tmp_path, config, strict=True)


def test_ignores_preprocess_without_loading_the_script(
    tmp_path: Path,
    capfd: pytest.CaptureFixture[str],
    caplog: pytest.LogCaptureFixture,
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Home\n\nOriginal content.\n", encoding="utf-8"
    )

    # Importing this script would fail the build, even without calling its hook.
    (tmp_path / "preprocess.py").write_text(
        'raise RuntimeError("Preprocess scripts must not be loaded")\n',
        encoding="utf-8",
    )
    config = dedent("""\
        plugins:
          - llmstxt:
              preprocess: preprocess.py
              full_output: llms-full.txt
              sections: {Pages: ["*.md"]}
    """)

    site = build(tmp_path, config, strict=True)

    assert (site / "index.md").read_text(
        encoding="utf-8"
    ) == "# Home\n\nOriginal content.\n"
    assert "Original content." in (site / "llms-full.txt").read_text(
        encoding="utf-8"
    )
    captured = capfd.readouterr()
    assert "preprocess" not in (captured.out + captured.err).lower()
    assert "preprocess" not in caplog.text.lower()


def test_disabled_plugin_emits_no_artifacts(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")

    site = build(
        tmp_path,
        dedent("""\
            plugins:
              - llmstxt: {enabled: false, sections: {Pages: [index.md]}}
        """),
    )

    assert not (site / "llms.txt").exists()
    assert not (site / "index.md").exists()


def test_fnmatch_patterns_cross_directories_but_keep_their_prefix(
    tmp_path: Path,
) -> None:
    docs = tmp_path / "docs"
    sources = {
        "guide/direct.md": "Direct",
        "guide/nested/deep.md": "Deep",
        "other/guide/wrong.md": "Wrong prefix",
        "guides/wrong.md": "Wrong directory",
    }
    for source, title in sources.items():
        file = docs / source
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(f"# {title}\n", encoding="utf-8")

    site = build(
        tmp_path,
        dedent("""\
            plugins:
              - llmstxt:
                  sections:
                    All guides: [guide/*.md]
                    Nested guides: [guide/**/*.md]
        """),
    )

    index = (site / "llms.txt").read_text(encoding="utf-8")
    assert index.count("[Direct]") == 1
    assert index.count("[Deep]") == 2
    assert "Wrong" not in index


@pytest.mark.parametrize("directory_urls", [True, False])
def test_toml_configuration_and_readable_output_urls(
    tmp_path: Path, directory_urls: bool
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        dedent("""\
            # Home

            [Café notes](caf%C3%A9%20notes.md)

            [Hash in filename](a%20%23%20caf%C3%A9.md)
        """),
        encoding="utf-8",
    )
    (docs / "café notes.md").write_text("# Café notes\n", encoding="utf-8")
    (docs / "a # café.md").write_text("# Heading #\n", encoding="utf-8")
    config = tmp_path / "zensical.toml"
    config.write_text(
        dedent(f"""\
            [project]
            site_name = "LLM test"
            site_url = "https://example.org/docs/"
            use_directory_urls = {str(directory_urls).lower()}

            [project.plugins.llmstxt.sections]
            Pages = ["index.md", "café notes.md", "a # café.md"]
        """),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    site = tmp_path / "site"
    suffix = "/index.md" if directory_urls else ".md"
    index = (site / "llms.txt").read_text(encoding="utf-8")
    page = (site / "index.md").read_text(encoding="utf-8")

    # Keep spaces and Unicode readable in both the index and converted links.
    page_suffix = "/index.md" if directory_urls else ".html"
    for markdown, link_suffix in ((index, suffix), (page, page_suffix)):
        assert (
            f"(<https://example.org/docs/café notes{link_suffix}>)" in markdown
        )

        # A literal # in the filename must not become a URL fragment.
        assert (
            f"(<https://example.org/docs/a %23 café{link_suffix}>)" in markdown
        )

    assert (site / f"café notes{suffix}").is_file()
    assert (site / f"a # café{suffix}").is_file()


def test_serve_updates_adds_renames_and_removes_markdown_outputs(
    tmp_path: Path,
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    source = docs / "post.md"
    source.write_text("# Original\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        dedent("""\
            site_name: Live LLM text
            site_url: https://example.org/
            dev_addr: 127.0.0.1:0
            plugins:
              - llmstxt:
                  full_output: llms-full.txt
                  sections: {Pages: ['*.md']}
        """),
        encoding="utf-8",
    )
    site = tmp_path / "site"
    log = (tmp_path / "serve.log").open("w+", encoding="utf-8")
    process = subprocess.Popen(  # noqa: S603  # Run this checkout's CLI on the test project.
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

    def wait_for(pages: dict[str, str], absent: tuple[str, ...] = ()) -> None:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            try:
                index = (site / "llms.txt").read_text(encoding="utf-8")
                full = (site / "llms-full.txt").read_text(encoding="utf-8")
                present = all(
                    f"[{title}]" in index
                    and f"# {title}\n" in full
                    and (site / name / "index.md").read_text(encoding="utf-8")
                    == f"# {title}\n"
                    for name, title in pages.items()
                )
                removed = all(
                    not (site / name / "index.md").exists()
                    and f"/{name}/index.md" not in index
                    for name in absent
                )
                if present and removed and index.count("- [") == len(pages):
                    return
            except OSError:
                pass
            if process.poll() is not None:
                break
            time.sleep(0.02)
        log.flush()
        log.seek(0)
        raise AssertionError(f"LLM text outputs did not settle: {log.read()}")

    try:
        wait_for({"post": "Original"})

        # Body changes update the per-page output and both aggregate files.
        source.write_text("# Revised\n", encoding="utf-8")
        wait_for({"post": "Revised"})

        added = docs / "added.md"
        added.write_text("# Added\n", encoding="utf-8")
        wait_for({"post": "Revised", "added": "Added"})

        # A renamed source retracts its previous Markdown destination.
        renamed = docs / "renamed.md"
        source.rename(renamed)
        wait_for({"renamed": "Revised", "added": "Added"}, ("post",))

        renamed.unlink()
        added.unlink()
        wait_for({}, ("post", "renamed", "added"))
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        log.close()
