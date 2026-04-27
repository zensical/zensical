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
from jinja2.exceptions import TemplateSyntaxError, UndefinedError
from markdown import Markdown

from zensical.extensions.context import (
    ContextExtension,
    ContextPreprocessor,
    Page,
)
from zensical.extensions.emoji import to_svg, twemoji
from zensical.extensions.macros import (
    MacroEnv,
    MacrosExtension,
    _fix_url,
    _format_value,
    _load_module,
    _load_one_yaml,
    _make_table,
    _merge_include_yaml,
    _pretty,
)

if TYPE_CHECKING:
    from pathlib import Path


MINIMAL_EXTENSIONS = {}

RECOMMENDED_EXTENSIONS = {
    "abbr": {},
    "admonition": {},
    "attr_list": {},
    "def_list": {},
    "footnotes": {},
    "md_in_html": {},
    "toc": {"permalink": True},
    "pymdownx.arithmatex": {"generic": True},
    "pymdownx.betterem": {},
    "pymdownx.caret": {},
    "pymdownx.details": {},
    "pymdownx.emoji": {
        "emoji_generator": to_svg,
        "emoji_index": twemoji,
    },
    "pymdownx.highlight": {
        "anchor_linenums": True,
        "line_spans": "__span",
        "pygments_lang_class": True,
    },
    "pymdownx.inlinehilite": {},
    "pymdownx.keys": {},
    "pymdownx.magiclink": {},
    "pymdownx.mark": {},
    "pymdownx.smartsymbols": {},
    "pymdownx.superfences": {
        "custom_fences": [{"name": "mermaid", "class": "mermaid"}]
    },
    "pymdownx.tabbed": {
        "alternate_style": True,
        "combine_header_slug": True,
    },
    "pymdownx.tasklist": {"custom_checkbox": True},
    "pymdownx.tilde": {},
}


def _page(meta: dict[str, Any] | None = None) -> dict[str, Any]:
    return {
        "url": "/",
        "path": "index.md",
        "meta": dict(meta or {}),
    }


@pytest.fixture(
    name="project_config",
    params=[MINIMAL_EXTENSIONS, RECOMMENDED_EXTENSIONS],
    ids=["minimal_markdown", "recommended_markdown"],
)
def _fixture_project_config(request: pytest.FixtureRequest) -> dict[str, Any]:
    active_markdown = dict(request.param)
    return {"site_name": "Demo", "markdown_extensions": active_markdown}


@pytest.fixture(name="md")
def _fixture_md(
    project_config: dict[str, Any],
    request: pytest.FixtureRequest,
    tmp_path: Path,
) -> Markdown:
    """Return a Markdown instance with MacrosExtension registered."""
    fixture_param = dict(getattr(request, "param", {}))
    allowed_keys = {"macros", "page", "project_config"}
    unexpected_keys = set(fixture_param) - allowed_keys
    if unexpected_keys:
        raise ValueError(
            f"Unsupported md fixture params: {sorted(unexpected_keys)}. "
            "Use only 'macros', 'page', and 'project_config'."
        )
    macro_config = dict(fixture_param.get("macros", {}))
    page = fixture_param.get("page", Page(url="/", path="index.md"))
    if isinstance(page, dict):
        page = Page(**page)
    project_overrides = dict(fixture_param.get("project_config", {}))
    effective_project_config = {**project_config, **project_overrides}
    if "root_dir" not in effective_project_config:
        effective_project_config["root_dir"] = str(tmp_path)
    markdown_extensions = dict(
        effective_project_config.get("markdown_extensions", {})
    )
    md = Markdown(
        extensions=list(markdown_extensions.keys()),
        extension_configs=markdown_extensions,
    )
    ContextExtension(page=page, config=effective_project_config).extendMarkdown(
        md
    )
    MacrosExtension(**macro_config).extendMarkdown(md)
    return md


@pytest.mark.parametrize(
    ("url", "expected"),
    [
        ("page.html", "../page.html"),
        ("assets/img.png", "../assets/img.png"),
        ("https://example.org", "https://example.org"),
        ("mailto:test@example.org", "mailto:test@example.org"),
    ],
)
def test_fix_url(url: str, expected: str) -> None:
    assert _fix_url(url) == expected


def test_macro_env_registers_macros_and_filters() -> None:
    env = MacroEnv()

    @env.macro
    def twice(value: int) -> int:
        return value * 2

    @env.filter(name="rev")
    def reverse(value: str) -> str:
        return value[::-1]

    assert env.macros["twice"](4) == 8
    assert env.filters["rev"]("abc") == "cba"


@pytest.mark.parametrize(
    ("payload", "expected"),
    [
        ([("alpha", "str", "hello")], "**alpha** | *str* | hello"),
        ([], ""),
    ],
)
def test_pretty(payload: list[tuple[str, str, str]], expected: str) -> None:
    output = _pretty(payload)
    if expected:
        assert expected in output
    else:
        assert output == ""


def test_make_table_escapes_pipe() -> None:
    table = _make_table(
        rows=[("left|right", "type", "a|b")],
        header=("Variable", "Type", "Content"),
    )
    assert "left\\|right" in table
    assert "a\\|b" in table


def test_format_value_for_callable_and_dict() -> None:
    def sample(name: str) -> str:
        """Doc first line.\nIgnored."""
        return name

    class Obj:
        pass

    value = {
        "count": 1,
        "name": "hello",
        "obj": Obj(),
    }

    rendered_callable = _format_value(sample)
    rendered_dict = _format_value(value)

    assert "(*name*)" in rendered_callable
    assert "Doc first line." in rendered_callable
    assert "**count** = 1" in rendered_dict
    assert "**obj** [*Obj*]" in rendered_dict


@pytest.mark.parametrize(
    ("relative_path", "content", "expected"),
    [
        ("ok.yaml", "a: 1\nb: x\n", {"a": 1, "b": "x"}),
        ("not_dict.yaml", "- 1\n- 2\n", None),
    ],
)
def test_load_one_yaml_with_relative_paths(
    tmp_path: Path,
    relative_path: str,
    content: str,
    expected: dict | None,
) -> None:
    (tmp_path / relative_path).write_text(content, encoding="utf-8")
    loaded = _load_one_yaml(relative_path, tmp_path)
    assert loaded == expected


def test_load_one_yaml_blocks_outside_project_root(tmp_path: Path) -> None:
    outside = tmp_path.parent / "outside.yaml"
    outside.write_text("x: 1\n", encoding="utf-8")
    loaded = _load_one_yaml(str(outside), tmp_path)
    assert loaded is None


@pytest.mark.parametrize(
    "include_yaml",
    [
        ["a.yaml", "b.yaml"],
        {"left": "a.yaml", "right": "b.yaml"},
    ],
)
def test_merge_include_yaml_list_and_dict(
    tmp_path: Path,
    include_yaml: list[str] | dict[str, str],
) -> None:
    (tmp_path / "a.yaml").write_text("x: 1\n", encoding="utf-8")
    (tmp_path / "b.yaml").write_text("y: 2\n", encoding="utf-8")
    variables: dict = {}
    _merge_include_yaml(include_yaml, tmp_path, variables)
    if isinstance(include_yaml, list):
        assert variables == {"x": 1, "y": 2}
    else:
        assert variables == {"left": {"x": 1}, "right": {"y": 2}}


def test_load_module_from_local_file(tmp_path: Path) -> None:
    (tmp_path / "main.py").write_text(
        "def define_env(env):\n"
        "    env.variables['site_name'] = 'Demo'\n"
        "    @env.macro\n"
        "    def twice(x):\n"
        "        return x * 2\n"
        "    @env.filter\n"
        "    def shout(s):\n"
        "        return s.upper()\n",
        encoding="utf-8",
    )
    variables, macros, filters = _load_module("main", tmp_path)
    assert variables["site_name"] == "Demo"
    assert macros["twice"](3) == 6
    assert filters["shout"]("hi") == "HI"


@pytest.mark.parametrize("module_name", ["../evil", "foo/bar", r"foo\\bar"])
def test_load_module_rejects_non_package_like_module_names(
    module_name: str,
) -> None:
    assert _load_module(module_name) == ({}, {}, {})


@pytest.mark.parametrize(
    ("md", "expected"),
    [
        ({"macros": {"render_by_default": False}}, "<p>Value: {{ 1 + 1 }}</p>"),
        ({"macros": {"render_by_default": True}}, "<p>Value: 2\n</p>"),
    ],
    indirect=["md"],
)
def test_preprocessor_respects_render_by_default(
    md: Markdown, expected: str
) -> None:
    source = "Value: {{ 1 + 1 }}"
    assert md.convert(source) == expected


@pytest.mark.parametrize(
    "md",
    [
        {
            "macros": {"render_by_default": False},
            "page": _page({"render_macros": True}),
        },
    ],
    indirect=True,
)
def test_preprocessor_renders_when_opted_in_by_page_meta(md: Markdown) -> None:
    assert md.convert("Value: {{ 1 + 1 }}") == "<p>Value: 2\n</p>"


@pytest.mark.parametrize(
    "md",
    [
        {
            "macros": {
                "render_by_default": True,
                "include_yaml": ["vars.yaml"],
                "module_name": "main",
            },
            "page": _page({"render_macros": True}),
            "project_config": {"extra": {"who": "world"}},
        },
    ],
    indirect=True,
)
def test_preprocessor_renders_with_include_yaml_and_module(
    md: Markdown,
    tmp_path: Path,
) -> None:
    (tmp_path / "vars.yaml").write_text("name: Ada\n", encoding="utf-8")
    (tmp_path / "main.py").write_text(
        "def define_env(env):\n"
        "    @env.macro\n"
        "    def greet(name):\n"
        "        return f'Hello {name}!'\n",
        encoding="utf-8",
    )
    rendered = md.convert("{{ greet(name) }}\n\n{{ who }}")
    assert "Hello Ada!" in rendered
    assert "world" in rendered


@pytest.mark.parametrize(
    "md",
    [{"macros": {"render_by_default": True}}],
    indirect=True,
)
def test_preprocessor_error_handling_keep_text_by_default(md: Markdown) -> None:
    assert "{{ not_closed" in md.convert("{{ not_closed")


@pytest.mark.parametrize(
    "md",
    [{"macros": {"render_by_default": True, "on_error_fail": True}}],
    indirect=True,
)
def test_preprocessor_error_handling_raises_when_enabled(md: Markdown) -> None:
    with pytest.raises(TemplateSyntaxError):
        md.convert("{{ not_closed")


@pytest.mark.parametrize(
    "md",
    [
        {
            "macros": {"render_by_default": True},
            "page": _page({"render_macros": True, "title": "Doc {{ 2 + 3 }}"}),
        },
    ],
    indirect=True,
)
def test_preprocessor_renders_jinja_in_title_meta(md: Markdown) -> None:
    md.convert("# {{ title }}")
    context = ContextPreprocessor.from_markdown(md)
    assert context is not None
    assert context.page.meta["title"] == "Doc 5"


@pytest.mark.parametrize(
    "md",
    [
        {
            "macros": {
                "render_by_default": True,
                "on_undefined": "strict",
                "on_error_fail": True,
            },
        }
    ],
    indirect=True,
)
def test_preprocessor_strict_undefined_raises(md: Markdown) -> None:
    with pytest.raises(UndefinedError):
        md.convert("{{ missing_variable }}")
