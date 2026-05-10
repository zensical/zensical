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

import pandas
import pytest
from jinja2.exceptions import TemplateSyntaxError, UndefinedError

from tests.unit.extensions.conftest import soup
from zensical.extensions.context import ContextPreprocessor
from zensical.extensions.macros import (
    MacroEnv,
    _add_indentation,
    _convert_to_md_table,
    _fix_url,
    _get_fake_table_readers,
    _get_table_readers,
    _load_module,
    _load_one_yaml,
    _merge_include_yaml,
    _pretty,
)

if TYPE_CHECKING:
    from pathlib import Path

    from markdown import Markdown
    from pandas import DataFrame


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
        ("md", "expected_text"),
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
                "Value: {{ 1 + 1 }}",
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
                "Value: 2",
                id="enabled_renders",
            ),
        ],
        indirect=["md"],
    )
    def test_respects_render_by_default(
        self, md: Markdown, expected_text: str
    ) -> None:
        html = soup(md.convert("Value: {{ 1 + 1 }}"))
        p = html.select_one("p")
        assert p is not None
        assert p.get_text() == expected_text

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
        html = soup(md.convert("Value: {{ 1 + 1 }}"))
        p = html.select_one("p")
        assert p is not None
        assert p.get_text() == "Value: 2"

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
        html = soup(md.convert("{{ greet(name) }}\n\n{{ who }}"))
        text = html.get_text()
        assert "Hello Ada!" in text
        assert "world" in text

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
        html = soup(md.convert("{{ not_closed"))
        assert "{{ not_closed" in html.get_text()

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
        html = soup(md.convert("{{ code_snippet('python', 'x = 1 + 1') }}"))
        code = html.select_one("code")
        assert code is not None
        # Language in class attr, not text
        assert "python" not in code.get_text()
        # Source broken into syntax-highlighted spans
        assert code.select("span")


# ---------------------------------------------------------------------------
# Table helpers
# ---------------------------------------------------------------------------


# Unit tests for the table conversion helper functions.
class TestTableHelpers:
    def test_add_indentation_spaces(self) -> None:
        result = _add_indentation("line1\nline2", spaces=4)
        assert result == "    line1\n    line2"

    def test_add_indentation_tabs(self) -> None:
        result = _add_indentation("line1\nline2", tabs=2)
        assert result == "\t\tline1\n\t\tline2"

    def test_add_indentation_none_returns_unchanged(self) -> None:
        assert _add_indentation("hello") == "hello"

    def test_add_indentation_raises_when_both_specified(self) -> None:
        with pytest.raises(ValueError, match="spaces or tabs"):
            _add_indentation("x", spaces=2, tabs=1)

    def test_convert_to_md_table_basic(self) -> None:
        df: DataFrame = pandas.DataFrame(
            {"Name": ["Alice", "Bob"], "Age": [30, 25]}
        )
        result = _convert_to_md_table(df)
        assert "|" in result
        assert "Name" in result
        assert "Age" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_convert_to_md_table_escapes_pipes_in_cells(self) -> None:
        df: DataFrame = pandas.DataFrame({"Col": ["a|b", "c"]})
        result = _convert_to_md_table(df)
        assert r"a\|b" in result

    def test_convert_to_md_table_escapes_pipes_in_column_names(self) -> None:
        df: DataFrame = pandas.DataFrame({"Na|me": ["Alice"]})
        result = _convert_to_md_table(df)
        assert r"Na\|me" in result

    def test_convert_to_md_table_omits_index_by_default(self) -> None:
        # Custom index values must not leak into the output.
        # Verifies that the `index=False` default is applied.
        df: DataFrame = pandas.DataFrame({"X": [1, 2]}, index=[100, 200])
        result = _convert_to_md_table(df)
        assert "100" not in result
        assert "200" not in result


# ---------------------------------------------------------------------------
# Table readers
# ---------------------------------------------------------------------------


class TestTableReaders:
    def test_fake_readers_raise_runtime_error_when_pandas_missing(
        self,
    ) -> None:
        readers = _get_fake_table_readers()
        for reader in readers.values():
            with pytest.raises(RuntimeError, match="table reading requires"):
                reader("irrelevant.csv")

    # CSV
    def test_read_csv(self, tmp_path: Path) -> None:
        (tmp_path / "data.csv").write_text(
            "Name,Age\nAlice,30\nBob,25\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_csv"]("data.csv")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_csv_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.csv").write_text("X,Y\n1,2\n3,4\n", encoding="utf-8")
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_csv"]("data.csv")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # JSON
    def test_read_json(self, tmp_path: Path) -> None:
        (tmp_path / "data.json").write_text(
            '[{"Name": "Alice", "Age": 30}, {"Name": "Bob", "Age": 25}]',
            encoding="utf-8",
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_json"]("data.json")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_json_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.json").write_text(
            '[{"X": 1, "Y": 2}, {"X": 3, "Y": 4}]', encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_json"]("data.json")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # YAML
    def test_read_yaml(self, tmp_path: Path) -> None:
        (tmp_path / "data.yaml").write_text(
            "- Name: Alice\n  Age: 30\n- Name: Bob\n  Age: 25\n",
            encoding="utf-8",
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_yaml"]("data.yaml")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_yaml_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.yaml").write_text(
            "- X: 1\n  Y: 2\n- X: 3\n  Y: 4\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_yaml"]("data.yaml")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # Table (tab-separated)
    def test_read_table(self, tmp_path: Path) -> None:
        (tmp_path / "data.tsv").write_text(
            "Name\tAge\nAlice\t30\nBob\t25\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_table"]("data.tsv")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_table_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.tsv").write_text(
            "X\tY\n1\t2\n3\t4\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_table"]("data.tsv")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # FWF (fixed-width format)
    def test_read_fwf(self, tmp_path: Path) -> None:
        content = "Name     Age\nAlice    30\nBob      25\n"
        (tmp_path / "data.fwf").write_text(content, encoding="utf-8")
        readers = _get_table_readers(tmp_path)
        result = readers["read_fwf"]("data.fwf")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_fwf_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.fwf").write_text(
            "X    Y\n1    2\n3    4\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_fwf"]("data.fwf")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # Excel (.xlsx)
    def test_read_excel(self, tmp_path: Path) -> None:
        pytest.importorskip("openpyxl")
        df: DataFrame = pandas.DataFrame(
            {"Name": ["Alice", "Bob"], "Age": [30, 25]}
        )
        df.to_excel(tmp_path / "data.xlsx", index=False)
        readers = _get_table_readers(tmp_path)
        result = readers["read_excel"]("data.xlsx")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_excel_returns_dataframe(self, tmp_path: Path) -> None:
        pytest.importorskip("openpyxl")
        df_in: DataFrame = pandas.DataFrame({"X": [1, 3], "Y": [2, 4]})
        df_in.to_excel(tmp_path / "data.xlsx", index=False)
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_excel"]("data.xlsx")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # Feather
    def test_read_feather(self, tmp_path: Path) -> None:
        pytest.importorskip("pyarrow")
        df: DataFrame = pandas.DataFrame(
            {"Name": ["Alice", "Bob"], "Age": [30, 25]}
        )
        df.to_feather(tmp_path / "data.feather")
        readers = _get_table_readers(tmp_path)
        result = readers["read_feather"]("data.feather")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_feather_returns_dataframe(self, tmp_path: Path) -> None:
        pytest.importorskip("pyarrow")
        df_in: DataFrame = pandas.DataFrame({"X": [1, 3], "Y": [2, 4]})
        df_in.to_feather(tmp_path / "data.feather")
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_feather"]("data.feather")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # End-to-end: CSV rendered through the Jinja2 / Markdown pipeline
    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                            },
                        },
                    }
                },
                id="end_to_end_csv",
            ),
        ],
        indirect=["md"],
    )
    def test_end_to_end_csv_via_template(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "scores.csv").write_text(
            "Player,Score\nAlice,100\nBob,80\n", encoding="utf-8"
        )
        html = soup(md.convert("{{ read_csv('scores.csv') }}"))
        table = html.select_one("table")
        assert table is not None
        headers = [th.get_text(strip=True) for th in table.select("th")]
        assert "Player" in headers
        assert "Score" in headers
        cells = [td.get_text(strip=True) for td in table.select("td")]
        assert "Alice" in cells

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                            },
                        },
                    }
                },
                id="end_to_end_pd_filter",
            ),
        ],
        indirect=["md"],
    )
    def test_end_to_end_pd_read_csv_with_convert_filter(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "nums.csv").write_text("X,Y\n1,2\n3,4\n", encoding="utf-8")
        html = soup(
            md.convert("{{ pd_read_csv('nums.csv') | convert_to_md_table }}")
        )
        table = html.select_one("table")
        assert table is not None
        headers = [th.get_text(strip=True) for th in table.select("th")]
        assert "X" in headers
        assert "Y" in headers
