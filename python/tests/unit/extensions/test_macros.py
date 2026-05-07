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

from typing import TYPE_CHECKING

import pytest
from jinja2.exceptions import TemplateSyntaxError, UndefinedError

from zensical.extensions.context import ContextPreprocessor
from zensical.extensions.macros import (
    MacroEnv,
    _fix_url,
    _load_module,
    _load_one_yaml,
    _merge_include_yaml,
    _pretty,
)

if TYPE_CHECKING:
    from pathlib import Path

    from markdown import Markdown


# ---------------------------------------------------------------------------
# Filters
# ---------------------------------------------------------------------------


class TestFilters:
    @pytest.mark.parametrize(
        ("url", "expected"),
        [
            pytest.param("page.html", "../page.html", id="relative_html"),
            pytest.param(
                "assets/image.png", "../assets/image.png", id="relative_asset"
            ),
            pytest.param(
                "https://example.org",
                "https://example.org",
                id="absolute_https",
            ),
            pytest.param(
                "mailto:test@example.org",
                "mailto:test@example.org",
                id="mailto",
            ),
        ],
    )
    def test_fix_url(self, url: str, expected: str) -> None:
        assert _fix_url(url) == expected

    @pytest.mark.parametrize(
        ("payload", "expected"),
        [
            pytest.param(
                [("alpha", "str", "hello")],
                "**alpha** | *str* | hello",
                id="single_row",
            ),
            pytest.param([], "", id="empty"),
        ],
    )
    def test_pretty(
        self, payload: list[tuple[str, str, str]], expected: str
    ) -> None:
        output = _pretty(payload)
        if expected:
            assert expected in output
        else:
            assert output == ""


# ---------------------------------------------------------------------------
# Defining environments
# ---------------------------------------------------------------------------


class TestMacroEnv:
    def test_registers_macros_and_filters(self) -> None:
        env = MacroEnv()

        @env.macro
        def twice(value: int) -> int:
            return value * 2

        @env.filter(name="rev")
        def reverse(value: str) -> str:
            return value[::-1]

        assert env.macros["twice"](4) == 8
        assert env.filters["rev"]("abc") == "cba"


# ---------------------------------------------------------------------------
# Loading YAML
# ---------------------------------------------------------------------------


class TestLoadYAML:
    @pytest.mark.parametrize(
        ("relative_path", "content", "expected"),
        [
            pytest.param(
                "ok.yaml", "a: 1\nb: x\n", {"a": 1, "b": "x"}, id="valid_dict"
            ),
            pytest.param(
                "not_dict.yaml", "- 1\n- 2\n", None, id="non_dict_returns_none"
            ),
        ],
    )
    def test_with_relative_paths(
        self,
        tmp_path: Path,
        relative_path: str,
        content: str,
        expected: dict | None,
    ) -> None:
        (tmp_path / relative_path).write_text(content, encoding="utf-8")
        loaded = _load_one_yaml(relative_path, tmp_path)
        assert loaded == expected

    def test_blocks_outside_project_root(self, tmp_path: Path) -> None:
        outside = tmp_path.parent / "outside.yaml"
        outside.write_text("x: 1\n", encoding="utf-8")
        loaded = _load_one_yaml(str(outside), tmp_path)
        assert loaded is None

    @pytest.mark.parametrize(
        "include_yaml",
        [
            pytest.param(["a.yaml", "b.yaml"], id="list"),
            pytest.param({"left": "a.yaml", "right": "b.yaml"}, id="dict"),
        ],
    )
    def test_merge_include(
        self,
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


# ---------------------------------------------------------------------------
# Loading modules / pluglets
# ---------------------------------------------------------------------------


class TestLoadModule:
    def test_from_local_file(self, tmp_path: Path) -> None:
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

    @pytest.mark.parametrize(
        "module_name",
        [
            pytest.param("../evil", id="path_traversal"),
            pytest.param("foo/bar", id="forward_slash"),
            pytest.param("foo\\\\bar", id="backslash"),
        ],
    )
    def test_rejects_invalid_names(self, module_name: str) -> None:
        assert _load_module(module_name) == ({}, {}, {})


# ---------------------------------------------------------------------------
# Markdown preprocessor
# ---------------------------------------------------------------------------


class TestPreprocessor:
    @pytest.mark.parametrize(
        ("md", "expected"),
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": False
                            },
                        },
                    }
                },
                "<p>Value: {{ 1 + 1 }}</p>",
                id="disabled_keeps_template",
            ),
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True
                            },
                        },
                    }
                },
                "<p>Value: 2</p>",
                id="enabled_renders",
            ),
        ],
        indirect=["md"],
    )
    def test_respects_render_by_default(
        self, md: Markdown, expected: str
    ) -> None:
        source = "Value: {{ 1 + 1 }}"
        assert md.convert(source) == expected

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": False
                            },
                        },
                    },
                    "page": {"meta": {"render_macros": True}},
                },
                id="page_opt_in",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_when_opted_in_by_page_meta(self, md: Markdown) -> None:
        assert md.convert("Value: {{ 1 + 1 }}") == "<p>Value: 2</p>"

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "include_yaml": ["vars.yaml"],
                                "module_name": "main",
                            },
                        },
                        "extra/who": "world",
                    },
                    "page": {"meta": {"render_macros": True}},
                },
                id="include_yaml_and_module",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_with_include_yaml_and_module(
        self,
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
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True
                            },
                        },
                    }
                },
                id="render_by_default",
            ),
        ],
        indirect=["md"],
    )
    def test_error_handling_keep_text_by_default(self, md: Markdown) -> None:
        assert "{{ not_closed" in md.convert("{{ not_closed")

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "on_error_fail": True,
                            },
                        },
                    }
                },
                id="on_error_fail",
            ),
        ],
        indirect=["md"],
    )
    def test_error_handling_raises_when_enabled(self, md: Markdown) -> None:
        with pytest.raises(TemplateSyntaxError):
            md.convert("{{ not_closed")

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True
                            },
                        },
                    },
                    "page": {
                        "meta": {
                            "render_macros": True,
                            "title": "Doc {{ 2 + 3 }}",
                        },
                    },
                },
                id="jinja_in_title",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_jinja_in_title_meta(self, md: Markdown) -> None:
        md.convert("# {{ title }}")
        context = ContextPreprocessor.from_markdown(md)
        assert context is not None
        assert context.page.meta["title"] == "Doc 5"

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "on_undefined": "strict",
                                "on_error_fail": True,
                            },
                        },
                    },
                },
                id="strict_on_error_fail",
            ),
        ],
        indirect=["md"],
    )
    def test_strict_undefined_raises(self, md: Markdown) -> None:
        with pytest.raises(UndefinedError):
            md.convert("{{ missing_variable }}")

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "module_name": "main",
                            },
                            "pymdownx.superfences": {},
                        },
                    },
                },
                id="superfences",
            ),
        ],
        indirect=["md"],
    )
    def test_fenced_code_block_processed_by_superfences(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    @env.macro\n"
            "    def code_snippet(lang, code):\n"
            "        return f'```{lang}\\n{code}\\n```\\n'\n",
            encoding="utf-8",
        )
        result = md.convert("{{ code_snippet('python', 'x = 1 + 1') }}")
        assert "<code>python" not in result
        assert "x = 1 + 1" not in result  # we expect spans
