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
from typing import TYPE_CHECKING, Any

import pytest
import yaml

import zensical.config as cfg_module
from zensical.config import (
    ConfigurationError,
    get_builtin_theme_dir,
    get_theme_dir,
    parse_config,
    parse_mkdocs_config,
)
from zensical.extensions.autorefs import AutorefsExtension
from zensical.extensions.glightbox import GlightboxExtension
from zensical.extensions.macros import MacrosExtension
from zensical.extensions.mkdocstrings import MkdocstringsExtension

if TYPE_CHECKING:
    from collections.abc import Generator


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _write_toml_config(tmp_path: Path, **project_keys: Any) -> Path:
    """Write a minimal `zensical.toml` with the given project keys."""
    docs_dir = project_keys.get("docs_dir", "docs")
    if docs_dir and not str(docs_dir).startswith(".."):
        tmp_path.joinpath(docs_dir).mkdir(exist_ok=True)

    lines = ["[project]", 'site_name = "test"']
    for key, value in project_keys.items():
        if isinstance(value, bool):
            formatted = "true" if value else "false"
        elif isinstance(value, (int, float)):
            formatted = str(value)
        else:
            formatted = f'"{value}"'
        lines.append(f"{key} = {formatted}")

    config_file = tmp_path / "zensical.toml"
    config_file.write_text("\n".join(lines) + "\n")
    return config_file


def _minimal_yaml(**extra_keys: object) -> str:
    """Build a minimal mkdocs.yml YAML string with the required scaffolding."""
    base: dict[str, Any] = {
        "site_name": "Test Site",
        "theme": {"name": "material"},
        "extra": {},
        "plugins": [],
        "mdx_configs": {},
    }
    base.update(extra_keys)
    return yaml.dump(base)


def _write_mkdocs_config(tmp_path: Path, yaml_str: str) -> Path:
    """Create the `docs` directory and write `mkdocs.yml` content."""
    tmp_path.joinpath("docs").mkdir()
    config_file = tmp_path / "mkdocs.yml"
    config_file.write_text(yaml_str)
    return config_file


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(autouse=True)
def _reset_config() -> Generator[None, None, None]:
    """Reset the global _CONFIG before and after each test."""
    cfg_module._CONFIG = None
    yield
    cfg_module._CONFIG = None


def test_site_dir_cant_be_empty(tmp_path: Path) -> None:
    config_file = _write_toml_config(tmp_path, site_dir="")
    with pytest.raises(ConfigurationError, match="empty"):
        parse_config(str(config_file))


def test_site_dir_cant_go_up(tmp_path: Path) -> None:
    config_file = _write_toml_config(tmp_path, site_dir="../site")
    with pytest.raises(ConfigurationError, match="within"):
        parse_config(str(config_file))


def test_docs_dir_cant_be_empty(tmp_path: Path) -> None:
    config_file = _write_toml_config(tmp_path, docs_dir="")
    with pytest.raises(ConfigurationError, match="empty"):
        parse_config(str(config_file))


def test_docs_dir_cant_go_up(tmp_path: Path) -> None:
    config_file = _write_toml_config(tmp_path, docs_dir="../docs")
    with pytest.raises(ConfigurationError, match="within"):
        parse_config(str(config_file))


def test_docs_dir_must_exist(tmp_path: Path) -> None:
    config_file = _write_toml_config(tmp_path, docs_dir="docs")
    # Remove the auto-created docs directory to trigger the existence check.
    tmp_path.joinpath("docs").rmdir()
    with pytest.raises(ConfigurationError, match="does not exist"):
        parse_config(str(config_file))


def test_site_dir_docs_dir_cant_be_equal(tmp_path: Path) -> None:
    config_file = _write_toml_config(tmp_path, site_dir="same", docs_dir="same")
    with pytest.raises(ConfigurationError, match="must be different"):
        parse_config(str(config_file))


# ---------------------------------------------------------------------------
# Plugins to Markdown extensions
# ---------------------------------------------------------------------------


class TestPluginShimming:
    """Test that MkDocs plugin entries are shimmed into Markdown extensions."""

    def _parse_yaml(
        self, tmp_path: Path, **yaml_overrides: object
    ) -> dict[str, Any]:
        """Write a minimal mkdocs.yml with overrides and return config."""
        yaml_str = _minimal_yaml(**yaml_overrides)
        config_file = _write_mkdocs_config(tmp_path, yaml_str)
        parse_mkdocs_config(str(config_file))
        config = cfg_module._CONFIG
        assert config is not None
        return config

    def test_plugins_as_yaml_list(self, tmp_path: Path) -> None:
        config = self._parse_yaml(tmp_path, plugins=["glightbox"])
        assert GlightboxExtension.name in config["markdown_extensions"]

    def test_plugins_as_yaml_dict(self, tmp_path: Path) -> None:
        config = self._parse_yaml(tmp_path, plugins={"glightbox": {}})
        assert GlightboxExtension.name in config["markdown_extensions"]

    def test_material_meta_plugin_is_normalized(self, tmp_path: Path) -> None:
        config = self._parse_yaml(
            tmp_path,
            plugins={"material/meta": {"meta_file": "defaults.yml"}},
        )
        assert config["plugins"]["meta"]["config"] == {
            "enabled": True,
            "meta_file": "defaults.yml",
        }

    def test_mike_plugin_defaults_with_versioned_build(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.setenv("MIKE_DOCS_VERSION", "0.3")
        config = self._parse_yaml(
            tmp_path,
            site_url="https://example.com",
            plugins=["mike"],
        )
        assert config["site_url"] == "https://example.com/0.3"
        assert config["plugins"]["mike"]["config"] == {
            "alias_type": "symlink",
            "redirect_template": None,
            "deploy_prefix": "",
            "canonical_version": None,
        }

    def test_glightbox_adds_extension_and_forwards_config(
        self, tmp_path: Path
    ) -> None:
        config = self._parse_yaml(
            tmp_path, plugins={"glightbox": {"loop": True}}
        )
        assert GlightboxExtension.name in config["markdown_extensions"]
        assert config["mdx_configs"][GlightboxExtension.name] == {"loop": True}

    def test_macros_plugin_shimmed(self, tmp_path: Path) -> None:
        config = self._parse_yaml(tmp_path, plugins={"macros": {}})
        assert MacrosExtension.name in config["markdown_extensions"]

    def test_autorefs_standalone(self, tmp_path: Path) -> None:
        config = self._parse_yaml(tmp_path, plugins={"autorefs": {}})
        assert AutorefsExtension.name in config["markdown_extensions"]

    def test_autorefs_disabled_not_added(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.setattr("zensical.config.find_spec", lambda _name: True)
        config = self._parse_yaml(
            tmp_path,
            plugins={
                "autorefs": {"enabled": False},
                "mkdocstrings": {"enabled": True},
            },
        )
        assert AutorefsExtension.name not in config["markdown_extensions"]

    def test_mkdocstrings_disabled_neither_added(self, tmp_path: Path) -> None:
        config = self._parse_yaml(
            tmp_path, plugins={"mkdocstrings": {"enabled": False}}
        )
        assert AutorefsExtension.name not in config["markdown_extensions"]
        assert MkdocstringsExtension.name not in config["markdown_extensions"]

    def test_mkdocstrings_enabled_autorefs_also_added(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.setattr("zensical.config.find_spec", lambda _name: True)
        config = self._parse_yaml(tmp_path, plugins={"mkdocstrings": {}})
        assert AutorefsExtension.name in config["markdown_extensions"]
        assert MkdocstringsExtension.name in config["markdown_extensions"]

    def test_mkdocstrings_not_installed_raises(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        monkeypatch.setattr("zensical.config.find_spec", lambda _name: None)
        yaml_str = _minimal_yaml(plugins={"mkdocstrings": {}})
        config_file = _write_mkdocs_config(tmp_path, yaml_str)
        with pytest.raises(ConfigurationError):
            parse_mkdocs_config(str(config_file))


# ---------------------------------------------------------------------------
# Getting theme information
# ---------------------------------------------------------------------------


class TestGetThemeDir:
    """Test theme directory resolution."""

    def test_builtin_theme_dir_exists(self) -> None:
        assert Path(get_builtin_theme_dir()).exists()

    def test_get_theme_dir_material(self) -> None:
        assert get_theme_dir("material") == get_builtin_theme_dir()

    def test_get_theme_dir_zensical(self) -> None:
        assert get_theme_dir("zensical") == get_builtin_theme_dir()

    def test_get_theme_dir_unknown_raises(self) -> None:
        with pytest.raises(ConfigurationError):
            get_theme_dir("definitely-not-a-real-theme-xyzzy")
