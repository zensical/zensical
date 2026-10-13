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

from typing import TYPE_CHECKING, Any

import pytest
import yaml
from markdown import Markdown
from pymdownx.snippets import SnippetMissingError, SnippetPreprocessor

import zensical.config as config_module
from zensical.config import ConfigurationError, parse_config
from zensical.extensions.context import ContextExtension, Page
from zensical.extensions.include_markdown import IncludeMarkdownExtension

if TYPE_CHECKING:
    from pathlib import Path


def _markdown(
    root: Path,
    options: dict[str, Any] | None = None,
    snippets: dict[str, Any] | None = None,
    *,
    page: str = "guides/index.md",
) -> Markdown:
    """Configure the plugin and page context for a temporary project."""
    config = {
        "root_dir": str(root),
        "docs_dir": "docs",
        "markdown_extensions": ["pymdownx.snippets", "pymdownx.superfences"],
        "mdx_configs": {"pymdownx.snippets": snippets or {}},
        "plugins": {"include-markdown": {"config": options or {}}},
    }
    config_module._shim_include_markdown(config)

    return Markdown(
        extensions=[
            ContextExtension(
                config=config, page=Page(path=page, url="guides/")
            ),
            *config["markdown_extensions"],
        ],
        extension_configs=config["mdx_configs"],
    )


def _native_markdown(
    root: Path, settings: dict[str, Any] | None = None
) -> Markdown:
    """Configure plain Snippets without the include-markdown adapter."""
    snippets = {
        "base_path": [str(root / "docs")],
        "restrict_base_path": False,
        "check_paths": True,
        "url_download": True,
        **(settings or {}),
    }
    return Markdown(
        extensions=["pymdownx.snippets", "pymdownx.superfences"],
        extension_configs={"pymdownx.snippets": snippets},
    )


@pytest.mark.parametrize("directive", ["include", "include-markdown"])
@pytest.mark.parametrize(
    "filename",
    ["shared.md", "shared file.md", "shared.md:section", "both'\"quotes.md"],
)
@pytest.mark.parametrize(
    ("options", "settings", "include_indent", "snippet_indent"),
    [
        ({}, {}, "", ""),
        ({}, {}, "    ", "    "),
        ({"preserve_includer_indent": False}, {}, "    ", ""),
        ({"dedent": True}, {"dedent_subsections": True}, "    ", "    "),
    ],
)
def test_include_tags_match_plain_snippets(
    tmp_path: Path,
    directive: str,
    filename: str,
    options: dict[str, Any],
    settings: dict[str, Any],
    include_indent: str,
    snippet_indent: str,
) -> None:
    # Paths, indentation, and mapped settings must survive
    # conversion to Snippets.
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / filename).write_text(
        "    Included\n    content\n\n", encoding="utf-8"
    )
    md = _markdown(tmp_path, options)
    native = _native_markdown(tmp_path, settings)
    escaped = filename.replace('"', '\\"')
    include_source = f'{include_indent}{{% {directive} "{escaped}" %}}'
    snippet_source = f"--8<--\n{snippet_indent}{filename}:1\n--8<--"

    included_lines = md.preprocessors["snippet"].run(include_source.split("\n"))
    snippet_lines = native.preprocessors["snippet"].run(
        snippet_source.split("\n")
    )

    # Raw lines are compared so whitespace differences cannot be hidden by HTML.
    assert included_lines == snippet_lines
    assert md.convert(include_source) == native.convert(snippet_source)


@pytest.mark.parametrize(
    "options",
    [
        'start="BEGIN" end="END"',
        'start="missing"',
        'end="missing"',
        'start="" end=""',
        r'start="<!--\n\tdébut 雪\n-->"',
        "preserve-includer-indent=false dedent=true",
        "encoding=ascii trailing-newlines=false recursive=false",
        'exclude="*.md" order="name"',
        "rewrite-relative-urls=true comments=true heading-offset=2",
    ],
)
def test_unsupported_options_match_plain_whole_file_snippets(
    tmp_path: Path, options: str
) -> None:
    (tmp_path / "docs").mkdir()
    content = "# Heading\n\nbeforeBEGINcaféENDafter\n\n[Link](./target.md)\n\n"
    (tmp_path / "docs" / "shared.md").write_text(content, encoding="utf-8")
    md = _markdown(tmp_path)
    native = _native_markdown(tmp_path)
    include_source = f'    {{% include "shared.md" {options} %}}'
    snippet_source = "--8<--\n    shared.md:1\n--8<--"

    included_lines = md.preprocessors["snippet"].run(include_source.split("\n"))
    snippet_lines = native.preprocessors["snippet"].run(
        snippet_source.split("\n")
    )

    # Unsupported arguments cannot introduce behavior beyond native Snippets.
    assert included_lines == snippet_lines
    assert md.convert(include_source) == native.convert(snippet_source)


@pytest.mark.parametrize("directive", ["include", "include-markdown"])
@pytest.mark.parametrize("quote", ["'", '"'])
def test_includes_docs_relative_paths(
    tmp_path: Path, directive: str, quote: str
) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared file.md").write_text(
        "**Included**", encoding="utf-8"
    )
    md = _markdown(tmp_path)

    html = md.convert(f"{{% {directive} {quote}shared file.md{quote} %}}")

    assert html == "<p><strong>Included</strong></p>"


def test_nested_includes_resolve_from_each_source(tmp_path: Path) -> None:
    (tmp_path / "docs" / "guides").mkdir(parents=True)
    (tmp_path / "docs" / "shared").mkdir()
    (tmp_path / "docs" / "shared" / "outer.md").write_text(
        '{% include-markdown "./inner.md" %}', encoding="utf-8"
    )
    (tmp_path / "docs" / "shared" / "inner.md").write_text(
        "Nested **content**", encoding="utf-8"
    )
    md = _markdown(tmp_path)

    html = md.convert('{% include-markdown "../shared/outer.md" %}')

    assert html == "<p>Nested <strong>content</strong></p>"

    # Nested paths must be converted relative to Snippets' configured base path.
    (tmp_path / "docs" / "shared" / "outer.md").write_text(
        '--8<-- "shared/inner.md:1"', encoding="utf-8"
    )
    native = _native_markdown(tmp_path)

    assert html == native.convert('--8<-- "shared/outer.md:1"')


def test_repeated_and_circular_includes(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text(
        'Content\n\n{% include "./shared.md" %}', encoding="utf-8"
    )
    md = _markdown(tmp_path)

    html = md.convert('{% include "shared.md" %}\n\n{% include "shared.md" %}')

    assert html.count("Content") == 2
    assert "{%" not in html

    # The same recursion guard must apply after every tag has been converted.
    (tmp_path / "docs" / "shared.md").write_text(
        'Content\n\n--8<-- "shared.md:1"', encoding="utf-8"
    )
    native = _native_markdown(tmp_path)

    assert html == native.convert(
        '--8<-- "shared.md:1"\n\n--8<-- "shared.md:1"'
    )


def test_multiline_tags_and_per_inclusion_options(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text("# Heading", encoding="utf-8")
    md = _markdown(tmp_path)

    html = md.convert(
        '{%\n include-markdown "shared.md"'
        '\n heading-offset=2\n start="%} ignored"\n%}'
    )

    assert html == "<h1>Heading</h1>"


def test_trailing_whitespace_does_not_break_generated_snippets(
    tmp_path: Path,
) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text("Included", encoding="utf-8")
    md = _markdown(tmp_path)

    html = md.convert('{% include "shared.md" %}  \t\n\nAfter')

    assert html == "<p>Included</p>\n<p>After</p>"


def test_quoted_absolute_paths_and_global_encoding(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    included = tmp_path / "both'\"quotes.txt"
    included.write_text("café", encoding="latin-1")
    md = _markdown(tmp_path, {"encoding": "latin-1"})
    escaped = str(included).replace('"', '\\"')

    html = md.convert(f'{{% include "{escaped}" %}}')

    assert html == "<p>café</p>"

    native = _native_markdown(tmp_path, {"encoding": "latin-1"})

    assert html == native.convert(f"--8<--\n{included}:1\n--8<--")


def test_macros_and_file_expansion_match_plain_snippets(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text(
        "{{ 2 + 3 }}", encoding="utf-8"
    )
    md = _markdown(tmp_path)
    md.registerExtensions(["zensical.extensions.macros"], {})
    native = _native_markdown(tmp_path)
    native.registerExtensions(["zensical.extensions.macros"], {})

    html = md.convert('{{ 1 + 2 }}\n\n{% include "shared.md" %}')
    native_html = native.convert('{{ 1 + 2 }}\n\n--8<-- "shared.md:1"')

    # Macros are processed before file expansion, as with native Snippets.
    assert html == native_html == "<p>3</p>\n<p>{{ 2 + 3 }}</p>"


@pytest.mark.parametrize("preserve", [True, False])
def test_global_indentation_and_dedenting(
    tmp_path: Path, preserve: bool
) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text(
        "    included\n    content", encoding="utf-8"
    )
    md = _markdown(
        tmp_path, {"dedent": True, "preserve_includer_indent": preserve}
    )

    html = md.convert('    {% include "shared.md" %}')

    assert ("<pre><code>" in html) is preserve
    assert "included\ncontent" in html


def test_explicit_default_delimiters_and_directives(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text("Included", encoding="utf-8")
    options = {
        "opening_tag": "{%",
        "closing_tag": "%}",
        "directives": {
            "include": "include",
            "include-markdown": "include-markdown",
        },
    }
    config = {"extra": {"polyfills": []}, "root_dir": str(tmp_path)}
    plugin = config_module._convert_plugins(
        {"include-markdown": options}, config
    )["include-markdown"]["config"]
    md = _markdown(tmp_path, plugin)

    html = md.convert('{% include-markdown "shared.md" %}')

    assert html == "<p>Included</p>"


@pytest.mark.parametrize(
    "options",
    [
        {"opening_tag": "{!"},
        {"closing_tag": "!}"},
        {"directives": {"include": "replace"}},
        {"directives": {"include-markdown": "include-md"}},
        {
            "opening_tag": "[(",
            "closing_tag": ")]",
            "directives": {
                "include": "read+file",
                "include-markdown": "read.md",
            },
        },
    ],
)
def test_custom_syntax_in_plugin_and_extension(
    tmp_path: Path, options: dict[str, Any]
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    included = docs / "shared.md"
    included.write_text("Included", encoding="utf-8")
    config = {"extra": {"polyfills": []}, "root_dir": str(tmp_path)}
    plugin = config_module._convert_plugins(
        {"include-markdown": options}, config
    )["include-markdown"]["config"]
    opening = options.get("opening_tag", "{%")
    closing = options.get("closing_tag", "%}")
    names = {
        "include": "include",
        "include-markdown": "include-markdown",
        **options.get("directives", {}),
    }

    md = _markdown(tmp_path, plugin)

    for directive in names.values():
        assert (
            md.convert(f'{opening}\n {directive} "shared.md"\n{closing}')
            == "<p>Included</p>"
        )

    # The same options are also accepted without a plugin configuration.
    md = Markdown(
        extensions=[IncludeMarkdownExtension.name],
        extension_configs={IncludeMarkdownExtension.name: options},
    )

    assert (
        md.convert(f'{opening} {names["include"]} "{included}" {closing}')
        == "<p>Included</p>"
    )


@pytest.mark.parametrize(
    "options",
    [{"opening_tag": ""}, {"closing_tag": ""}, {"directives": {"include": ""}}],
)
def test_rejects_empty_syntax_in_direct_extension_configuration(
    tmp_path: Path, options: dict[str, Any]
) -> None:
    (tmp_path / "docs").mkdir()
    config_file = tmp_path / "mkdocs.yml"
    config_file.write_text(
        yaml.safe_dump(
            {
                "site_name": "Test",
                "markdown_extensions": {IncludeMarkdownExtension.name: options},
            }
        ),
        encoding="utf-8",
    )

    with pytest.raises(ConfigurationError, match="include-markdown"):
        parse_config(str(config_file))


def test_includes_inside_fenced_code(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "example.txt").write_text("<sample>", encoding="utf-8")
    md = _markdown(tmp_path)

    html = md.convert('```text\n{% include "example.txt" %}\n```')

    assert "&lt;sample&gt;" in html
    assert "{%" not in html


def test_native_snippets_and_auto_append_run_once(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "shared.md").write_text("Included", encoding="utf-8")
    (docs / "footer.md").write_text("Footer", encoding="utf-8")
    md = _markdown(tmp_path, snippets={"auto_append": ["footer.md"]})

    html = md.convert('--8<-- "shared.md"\n\n;--8<-- "escaped.md"')

    assert html.count("Included") == 1
    assert html.count("Footer") == 1
    assert '"escaped.md"' in html

    native = _native_markdown(tmp_path, {"auto_append": ["footer.md"]})

    assert html == native.convert('--8<-- "shared.md"\n\n;--8<-- "escaped.md"')


def test_remote_nested_includes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    downloads = []
    outer = '{% include "./inner.md" %}'

    def download(_self: SnippetPreprocessor, url: str) -> list[str]:
        downloads.append(url)
        return [outer] if url.endswith("outer.md") else ["Remote content"]

    # Network access is replaced after Snippets initializes its download cache.
    md = _markdown(tmp_path)
    native = _native_markdown(tmp_path)
    monkeypatch.setattr(SnippetPreprocessor, "download", download)

    html = md.convert('{% include "https://example.com/docs/outer.md" %}')

    assert html == "<p>Remote content</p>"
    assert downloads == [
        "https://example.com/docs/outer.md",
        "https://example.com/docs/inner.md",
    ]

    # Native Snippets requires complete URLs in downloaded snippets.
    outer = '--8<-- "https://example.com/docs/inner.md:1"'
    downloads.clear()

    native_html = native.convert('--8<-- "https://example.com/docs/outer.md:1"')

    assert html == native_html
    assert downloads == [
        "https://example.com/docs/outer.md",
        "https://example.com/docs/inner.md",
    ]


def test_explicit_snippets_dedenting_survives_adapter_removal(
    tmp_path: Path,
) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text(
        "    Included\n    content", encoding="utf-8"
    )
    md = _markdown(
        tmp_path, {"dedent": True}, snippets={"dedent_subsections": False}
    )
    native = _native_markdown(tmp_path, {"dedent_subsections": False})

    html = md.convert('{% include "shared.md" %}')
    native_html = native.convert('--8<-- "shared.md:1"')

    # Explicit native settings take precedence over mapped plugin defaults.
    assert html == native_html
    assert "<pre><code>" in html


@pytest.mark.parametrize("selection", ["2:3", "named"])
def test_native_line_ranges_and_sections_remain_unchanged(
    tmp_path: Path, selection: str
) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "shared.md").write_text(
        "Before\n--8<-- [start:named]\nSelected\n--8<-- [end:named]\nAfter",
        encoding="utf-8",
    )
    md = _markdown(tmp_path)
    native = _native_markdown(tmp_path)
    source = f'--8<-- "shared.md:{selection}"'

    html = md.convert(source)
    native_html = native.convert(source)

    assert html == native_html == "<p>Selected</p>"


def test_missing_files_fail(tmp_path: Path) -> None:
    md = _markdown(tmp_path)

    with pytest.raises(SnippetMissingError):
        md.convert('{% include "missing.md" %}')


def test_remote_includes_can_be_disabled(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    md = _markdown(tmp_path, {"include_from_url": False})

    def download(_self: SnippetPreprocessor, _url: str) -> list[str]:
        pytest.fail("Downloads must remain disabled")

    monkeypatch.setattr(SnippetPreprocessor, "download", download)

    with pytest.raises(SnippetMissingError):
        md.convert('{% include "https://example.com/file.md" %}')


@pytest.mark.parametrize(
    "tag",
    [
        "{% include filename %}",
        '{% include "file.md" unknown=true %}',
        'Before {% include "file.md" %} after',
    ],
)
def test_invalid_or_inline_tags_fail(tmp_path: Path, tag: str) -> None:
    md = _markdown(tmp_path)

    with pytest.raises(ValueError, match="include-markdown"):
        md.convert(tag)


def test_explicit_snippets_settings_take_precedence(tmp_path: Path) -> None:
    (tmp_path / "docs").mkdir()
    (tmp_path / "outside.md").write_text("Outside", encoding="utf-8")
    md = _markdown(tmp_path, snippets={"restrict_base_path": True})

    with pytest.raises(SnippetMissingError):
        md.convert('{% include "../../outside.md" %}')


@pytest.mark.parametrize("config_format", ["toml", "yaml"])
def test_plugin_shim_from_project_configuration(
    tmp_path: Path, config_format: str
) -> None:
    (tmp_path / "docs").mkdir()
    if config_format == "toml":
        config_file = tmp_path / "zensical.toml"
        config_file.write_text(
            '[project]\nsite_name = "Test"\n'
            '[project.plugins.include-markdown]\nencoding = "ascii"\n',
            encoding="utf-8",
        )
    else:
        config_file = tmp_path / "mkdocs.yml"
        config_file.write_text(
            "site_name: Test\nplugins:\n  - include-markdown:"
            "\n      encoding: ascii\n",
            encoding="utf-8",
        )

    config = parse_config(str(config_file))

    assert IncludeMarkdownExtension.name in config["markdown_extensions"]
    assert config["mdx_configs"]["pymdownx.snippets"]["encoding"] == "ascii"


def test_disabled_plugin_leaves_tags_untouched(tmp_path: Path) -> None:
    md = _markdown(tmp_path, {"enabled": False})

    html = md.convert('{% include "missing.md" %}')

    assert '{% include "missing.md" %}' in html


@pytest.mark.parametrize(
    ("option", "value"),
    [
        ("enabled", "yes"),
        ("dedent", 1),
        ("include_from_url", "true"),
        ("preserve_includer_indent", None),
        ("encoding", 42),
        ("opening_tag", ""),
        ("closing_tag", []),
        ("directives", []),
        ("directives", {"unknown": "name"}),
        ("directives", {"include": ""}),
        ("directives", {"include": "two words"}),
    ],
)
def test_rejects_invalid_configuration(option: str, value: Any) -> None:
    with pytest.raises(ConfigurationError, match="include-markdown"):
        config_module._convert_plugins(
            {"include-markdown": {option: value}},
            {"extra": {"polyfills": []}, "root_dir": "."},
        )


@pytest.mark.parametrize(
    "option",
    [
        "trailing_newlines",
        "comments",
        "rewrite_relative_urls",
        "heading_offset",
        "order",
        "start",
        "end",
        "exclude",
        "cache",
        "cache_dir",
        "recursive",
    ],
)
def test_unsupported_global_options_are_discarded(option: str) -> None:
    config = {"extra": {"polyfills": []}, "root_dir": "."}

    plugins = config_module._convert_plugins(
        {"include-markdown": {option: object()}}, config
    )

    assert plugins["include-markdown"]["config"] == {}
