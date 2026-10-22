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

from pathlib import Path

import pytest
from markdown import Markdown

import zensical


def _build_pages(
    tmp_path: Path,
    pages: dict[str, str],
    directory_urls: bool,
    extra_config: str = "",
) -> dict[str, str]:
    """Build pages and return their rendered content without theme markup."""
    docs = tmp_path / "docs"
    docs.mkdir()
    for source, content in pages.items():
        path = docs / source
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    overrides = tmp_path / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(
        "{{ page.content | safe }}", encoding="utf-8"
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_name = "Links"\n'
        'site_url = "https://example.org/docs/"\n'
        f"use_directory_urls = {str(directory_urls).lower()}\n"
        '[project.theme]\ncustom_dir = "overrides"\n' + extra_config,
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    rendered = {}
    for source in pages:
        path = Path(source)
        if path.name in {"index.md", "README.md"}:
            output = path.parent / "index.html"
        elif directory_urls:
            output = path.with_suffix("") / "index.html"
        else:
            output = path.with_suffix(".html")
        rendered[source] = (
            (tmp_path / "site" / output).read_text(encoding="utf-8").strip()
        )
    return rendered


@pytest.mark.parametrize("directory_urls", [True, False])
@pytest.mark.parametrize("source", ["guide/index.md", "guide/page.md"])
def test_markdown_and_raw_html_links_use_the_source_directory(
    tmp_path: Path, directory_urls: bool, source: str
) -> None:
    content = "\n\n".join(
        form.format(suffix=suffix)
        for suffix in ("", "#anchor", "?view=full#anchor")
        for form in (
            "[hello](foo/bar/hello.md/{suffix})",
            '<div><a href="foo/bar/hello.md/{suffix}">hello</a></div>',
        )
    )
    content += (
        "\n\n[reference][target]\n\n[target]: other.md/\n\n"
        "[Escaped](guide\\.md)"
    )
    content += (
        '\n\n<picture><source srcset="image.svg">'
        '<img src="image.svg"></picture>'
    )
    content += (
        '\n\n[Query](?diff1) <a href="?diff1#guide">'
        "Query</a> [Fragment](#guide)"
    )

    rendered = _build_pages(tmp_path, {source: content}, directory_urls)[source]

    prefix = "../" if directory_urls and source == "guide/page.md" else ""
    target = "foo/bar/hello/" if directory_urls else "foo/bar/hello.html"
    for suffix in ("", "#anchor", "?view=full#anchor"):
        assert rendered.count(f'href="{prefix}{target}{suffix}"') == 2
    other = "other/" if directory_urls else "other.html"
    guide = "guide/" if directory_urls else "guide.html"
    assert f'href="{prefix}{other}"' in rendered
    assert f'href="{prefix}{guide}"' in rendered
    assert f'srcset="{prefix}image.svg"' in rendered
    assert f'src="{prefix}image.svg"' in rendered
    assert 'href="?diff1"' in rendered
    assert 'href="?diff1#guide"' in rendered
    assert 'href="#guide"' in rendered


@pytest.mark.parametrize("highlight", [True, False])
def test_url_rewriting_preserves_code_and_html_text(
    tmp_path: Path, highlight: bool
) -> None:
    sources = [
        '```\ndocker run --mount src="$(pwd)",'
        'target="/src",type=bind image\n```',
        "```python\nself.click('a[href=\"?diff1\"]')\n```",
        '`src="$(pwd)"` and `a[href="?diff1"]`',
        '<pre><code>src="$(pwd)" and href="target.md"</code></pre>',
        '<!-- <img src="image.png"> href="target.md" -->',
        "<script>const example = '<a href=\"target.md\">';</script>",
        '<style>/* <img src="image.png"> */ '
        'a[href="target.md"] { color: red; }</style>',
        '<textarea><img src="image.png"> href="target.md"</textarea>',
        '<title>href="target.md"</title>',
        '<div data-href="target.md" data-example=\'src="image.png"\'>'
        'src="image.png"</div>',
    ]
    pages = {
        f"sample-{index}.md": source for index, source in enumerate(sources)
    }
    extensions = (
        '[project.markdown_extensions."pymdownx.highlight"]\n'
        f"use_pygments = {str(highlight).lower()}\n"
        '[project.markdown_extensions."pymdownx.superfences"]\n'
    )

    rendered = _build_pages(tmp_path, pages, True, extensions)

    # Rendered examples are compared byte for byte with Python-Markdown output.
    for path, source in pages.items():
        baseline = Markdown(
            extensions=["pymdownx.highlight", "pymdownx.superfences"],
            extension_configs={
                "pymdownx.highlight": {"use_pygments": highlight}
            },
        )
        assert rendered[path] == baseline.convert(source)


def test_nested_markdown_and_toc_links_are_rewritten_once(
    tmp_path: Path,
) -> None:
    content = (
        "# One\n\n## Two\n\n## Three\n\n"
        '[Normal](other.md)\n\n<a href="other.md">Raw</a>\n\n'
        '<div markdown="1">\n[Inner](other.md)\n</div>\n'
    )
    extensions = (
        "[project.markdown_extensions.toc]\n"
        "[project.markdown_extensions.md_in_html]\n"
    )

    rendered = _build_pages(
        tmp_path, {"guide/page.md": content}, True, extensions
    )["guide/page.md"]

    assert rendered.count('href="../other/"') == 3
    assert "../../other/" not in rendered


def test_preview_uses_rendering_context_before_rust_resolves_links(
    tmp_path: Path,
) -> None:
    extensions = (
        '[project.markdown_extensions."zensical.extensions.preview".sources]\n'
        'include = ["guide/*"]\n'
        '[project.markdown_extensions."zensical.extensions.preview".targets]\n'
        'include = ["guide/*"]\n'
    )

    rendered = _build_pages(
        tmp_path, {"guide/page.md": "[Other](other.md)"}, True, extensions
    )["guide/page.md"]

    assert 'data-preview=""' in rendered
    assert 'href="../other/"' in rendered


@pytest.mark.parametrize("directory_urls", [True, False])
@pytest.mark.parametrize("source", ["guide/index.md", "guide/page.md"])
def test_ampersands_in_markdown_link_targets(
    tmp_path: Path, directory_urls: bool, source: str
) -> None:
    # The same existing page is linked with literal and percent-encoded names.
    pages = {
        source: (
            "[Ampersand](<Lecture & Notes.md>)\n\n"
            "[Percent encoded](<Lecture%20%26%20Notes.md>)\n\n"
            '<a href="Lecture &#38; Notes.md?x=1&amp;y=2#part">Raw</a>'
        ),
        "guide/Lecture & Notes.md": "# Notes\n",
    }

    rendered = _build_pages(tmp_path, pages, directory_urls)[source]

    # Each target is rewritten to the route of the emitted page.
    prefix = "../" if directory_urls and source == "guide/page.md" else ""
    suffix = "/" if directory_urls else ".html"
    assert f'href="{prefix}Lecture &amp; Notes{suffix}"' in rendered
    assert f'href="{prefix}Lecture%20%26%20Notes{suffix}"' in rendered
    raw_target = f'href="{prefix}Lecture &amp; Notes{suffix}?x=1&amp;y=2#part"'
    assert raw_target in rendered


def test_media_urls_are_rewritten_after_all_markdown_postprocessors(
    tmp_path: Path,
) -> None:
    extensions = '[project.markdown_extensions."zensical.extensions.media"]\n'
    content = (
        '<p><img alt="type:audio" src="sound.mp3"></p>\n\n'
        "![type:video](movie.mp4)"
    )

    rendered = _build_pages(
        tmp_path, {"guide/page.md": content}, True, extensions
    )["guide/page.md"]

    assert 'src="../sound.mp3"' in rendered
    assert 'src="../movie.mp4"' in rendered


@pytest.mark.parametrize("directory_urls", [True, False])
def test_plugin_visitors_receive_rewritten_urls(
    tmp_path: Path, directory_urls: bool
) -> None:
    # A relative image is included in an autoref title, which is extracted
    # from the HTML and restored when references are resolved.
    source = "guide/page.md"
    pages = {
        source: (
            '[Other](other.md)\n\n'
            '<autoref identifier="target">'
            '<img src="icon.svg" alt="Icon">'
            '</autoref>'
        ),
        "guide/other.md": "# Target {#target}\n",
    }
    extra_config = (
        '[project.markdown_extensions.attr_list]\n'
        '[project.markdown_extensions."zensical.extensions.autorefs"]\n'
        '[project.plugins.llmstxt]\n'
        'autoclean = false\n'
        '[project.plugins.llmstxt.sections]\n'
        'Pages = ["guide/page.md"]\n'
    )

    html = _build_pages(tmp_path, pages, directory_urls, extra_config)[source]

    # The rewritten image URL is retained when the autoref title is restored.
    prefix = "../" if directory_urls else ""
    target = "other/" if directory_urls else "other.html"
    assert f'href="{prefix}{target}#target"' in html
    assert f'<img src="{prefix}icon.svg" alt="Icon">' in html
    assert "<autoref" not in html

    # Final page-relative URLs are also used in the Markdown export.
    output = "guide/page/index.md" if directory_urls else "guide/page.md"
    markdown = (tmp_path / "site" / output).read_text(encoding="utf-8")
    exported_target = "other/index.md" if directory_urls else "other.html"
    assert f"https://example.org/docs/guide/{exported_target}" in markdown
    assert "https://example.org/docs/guide/icon.svg" in markdown
