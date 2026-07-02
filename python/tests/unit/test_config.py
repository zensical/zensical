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

from textwrap import dedent
from typing import TYPE_CHECKING

import pytest

from zensical.config import ConfigurationError, parse_config

if TYPE_CHECKING:
    from pathlib import Path


def test_site_dir_cant_be_empty(tmp_path: Path) -> None:
    tmp_path.joinpath("docs").mkdir()
    config_file = tmp_path / "zensical.toml"
    config_file.write_text(
        dedent("""
            [project]
            site_name = "test"
            site_dir = ""
            """)
    )
    with pytest.raises(ConfigurationError, match="empty"):
        parse_config(str(config_file))


def test_site_dir_cant_go_up(tmp_path: Path) -> None:
    tmp_path.joinpath("docs").mkdir()
    config_file = tmp_path / "zensical.toml"
    config_file.write_text(
        dedent("""
            [project]
            site_name = "test"
            site_dir = "../site"
            """)
    )
    with pytest.raises(ConfigurationError, match="within"):
        parse_config(str(config_file))


def test_docs_dir_cant_be_empty(tmp_path: Path) -> None:
    tmp_path.joinpath("docs").mkdir()
    config_file = tmp_path / "zensical.toml"
    config_file.write_text(
        dedent("""
            [project]
            site_name = "test"
            docs_dir = ""
            """)
    )
    with pytest.raises(ConfigurationError, match="empty"):
        parse_config(str(config_file))


def test_docs_dir_cant_go_up(tmp_path: Path) -> None:
    tmp_path.joinpath("docs").mkdir()
    config_file = tmp_path / "zensical.toml"
    config_file.write_text(
        dedent("""
            [project]
            site_name = "test"
            docs_dir = "../docs"
            """)
    )
    with pytest.raises(ConfigurationError, match="within"):
        parse_config(str(config_file))


def test_docs_dir_must_exist(tmp_path: Path) -> None:
    config_file = tmp_path / "zensical.toml"
    config_file.write_text(
        dedent("""
            [project]
            site_name = "test"
            docs_dir = "docs"
            """)
    )
    with pytest.raises(ConfigurationError, match="does not exist"):
        parse_config(str(config_file))


def test_site_dir_docs_dir_cant_be_equal(tmp_path: Path) -> None:
    tmp_path.joinpath("docs").mkdir()
    config_file = tmp_path / "zensical.toml"
    config_file.write_text(
        dedent("""
            [project]
            site_name = "test"
            site_dir = "same"
            docs_dir = "same"
            """)
    )
    with pytest.raises(ConfigurationError, match="must be different"):
        parse_config(str(config_file))
