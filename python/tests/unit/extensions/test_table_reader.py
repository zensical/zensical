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

import logging
from typing import TYPE_CHECKING

import pytest

from tests.unit.extensions.conftest import soup
from zensical.extensions.table_reader import _parse_arguments

if TYPE_CHECKING:
    from pathlib import Path

    from markdown import Markdown


# -----------------------------------------------------------------------------
# Standalone tag parsing
# -----------------------------------------------------------------------------


class TestArgumentParsing:
    def test_parses_literal_arguments(self) -> None:
        args, kwargs = _parse_arguments(
            "'table.csv', sep=';', names=['A', 'B'], index=True"
        )
        assert args == ["table.csv"]
        assert kwargs == {
            "sep": ";",
            "names": ["A", "B"],
            "index": True,
        }

    @pytest.mark.parametrize(
        "arguments",
        [
            pytest.param("table_name", id="variable"),
            pytest.param("get_path()", id="call"),
            pytest.param("'table.csv', **options", id="keyword_expansion"),
        ],
    )
    def test_rejects_non_literal_arguments(self, arguments: str) -> None:
        with pytest.raises(ValueError, match="safely parse"):
            _parse_arguments(arguments)


# -----------------------------------------------------------------------------
# Markdown extension
# -----------------------------------------------------------------------------


class TestTableReaderExtension:
    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {},
                        },
                    }
                },
                id="table_reader",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_csv_without_rendering_general_jinja(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "people.csv").write_text(
            "Name,Age\nAlice,30\nBob,25\n", encoding="utf-8"
        )
        (tmp_path / "main.py").write_text(
            'raise RuntimeError("table-reader loaded macros")\n',
            encoding="utf-8",
        )
        html = soup(
            md.convert(
                "{{ read_csv('people.csv') }}\n\n"
                "General expression: {{ 1 + 1 }}"
            )
        )
        table = html.select_one("table")
        assert table is not None
        assert "Alice" in table.get_text()
        assert "{{ 1 + 1 }}" in html.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {
                                "data_path": "tables",
                            },
                        },
                    }
                },
                id="data_path",
            ),
        ],
        indirect=["md"],
    )
    def test_uses_data_path_and_project_root_precedence(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "tables").mkdir()
        (tmp_path / "docs" / "tables").mkdir(parents=True)
        (tmp_path / "tables" / "values.csv").write_text(
            "Value\nproject\n", encoding="utf-8"
        )
        (tmp_path / "docs" / "tables" / "values.csv").write_text(
            "Value\ndocs\n", encoding="utf-8"
        )
        html = soup(md.convert("{{ read_csv('values.csv') }}"))
        assert "project" in html.get_text()
        assert "docs" not in html.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {},
                        },
                    },
                    "page": {"path": "guide/page.md"},
                },
                id="page_relative",
            ),
        ],
        indirect=["md"],
    )
    def test_falls_back_to_page_directory(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        page_dir = tmp_path / "docs" / "guide"
        page_dir.mkdir(parents=True)
        (page_dir / "values.csv").write_text("Value\npage\n", encoding="utf-8")
        html = soup(md.convert("{{ read_csv('values.csv') }}"))
        assert "page" in html.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {
                                "allow_missing_files": True,
                            },
                        },
                    }
                },
                id="allow_missing",
            ),
        ],
        indirect=["md"],
    )
    def test_allows_missing_files_with_warning(
        self, md: Markdown, caplog: pytest.LogCaptureFixture
    ) -> None:
        with caplog.at_level(
            logging.WARNING,
            logger="zensical.extensions.table_reader",
        ):
            html = soup(md.convert("{{ read_csv('missing.csv') }}"))
        assert "Cannot find 'missing.csv'" in html.get_text()
        assert "Cannot find table file 'missing.csv'" in caplog.text

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {},
                        },
                    }
                },
                id="missing_fails",
            ),
        ],
        indirect=["md"],
    )
    def test_missing_files_fail_by_default(self, md: Markdown) -> None:
        with pytest.raises(FileNotFoundError, match=r"missing\.csv"):
            md.convert("{{ read_csv('missing.csv') }}")

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {
                                "select_readers": ["read_csv"],
                            },
                        },
                    }
                },
                id="selected_readers",
            ),
        ],
        indirect=["md"],
    )
    def test_only_replaces_selected_readers(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "values.csv").write_text(
            "Value\nselected\n", encoding="utf-8"
        )
        html = soup(
            md.convert(
                "{{ read_csv('values.csv') }}\n\n{{ read_json('values.json') }}"
            )
        )
        assert "selected" in html.get_text()
        assert "{{ read_json('values.json') }}" in html.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {},
                        },
                    }
                },
                id="read_raw",
            ),
        ],
        indirect=["md"],
    )
    def test_reads_raw_markdown(self, md: Markdown, tmp_path: Path) -> None:
        (tmp_path / "table.md").write_text(
            "Name | Value\n--- | ---\nRaw | 1\n", encoding="utf-8"
        )
        html = soup(md.convert("{{ read_raw('table.md') }}"))
        table = html.select_one("table")
        assert table is not None
        assert "Raw" in table.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {},
                        },
                    }
                },
                id="named_filepath",
            ),
        ],
        indirect=["md"],
    )
    def test_accepts_named_filepath(self, md: Markdown, tmp_path: Path) -> None:
        (tmp_path / "values.csv").write_text("Value\nnamed\n", encoding="utf-8")
        html = soup(
            md.convert("{{ read_csv(filepath_or_buffer='values.csv') }}")
        )
        assert "named" in html.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "module_name": "",
                            },
                        },
                        "plugins": {
                            "table-reader": {
                                "config": {
                                    "enabled": True,
                                    "data_path": "tables",
                                    "allow_missing_files": False,
                                    "select_readers": ["read_raw"],
                                },
                            },
                        },
                    }
                },
                id="macros_integration",
            ),
        ],
        indirect=["md"],
    )
    def test_macros_reuse_configured_readers(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "tables").mkdir()
        (tmp_path / "tables" / "values.csv").write_text(
            "Value\nmacros\n", encoding="utf-8"
        )
        html = soup(md.convert("{{ read_csv('values.csv') }}\n\n{{ 1 + 1 }}"))
        assert "macros" in html.get_text()
        assert "2" in html.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.table_reader": {},
                        },
                    }
                },
                id="project_boundary",
            ),
        ],
        indirect=["md"],
    )
    def test_rejects_files_outside_project(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        outside = tmp_path.parent / "outside.csv"
        outside.write_text("Value\noutside\n", encoding="utf-8")
        with pytest.raises(ValueError, match="within project root"):
            md.convert("{{ read_csv('../outside.csv') }}")
