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

# Integration tests for config loading: full round-trip via `zensical.build()`.
#
# Each test creates a minimal project in a fresh temp directory and invokes
# `zensical.build()`, which calls `Config::new()` on the Rust side. The
# assertions target the *side-effects* of config loading
# (e.g. theme loading: both config formats, success and error cases).

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import pytest

import zensical
import zensical.config as cfg_module
from zensical.config import ConfigurationError

if TYPE_CHECKING:
    from pathlib import Path


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


_BUILD_OPTS: dict[str, Any] = {"clean": False, "strict": False}


def _build(config_path: Path) -> None:
    """Invoke `zensical.build()` with standard (non-strict) options."""
    zensical.build(str(config_path), _BUILD_OPTS)


def _make_toml_project(
    tmp_path: Path,
    *,
    toml_extra: str = "",
) -> Path:
    """Scaffold a minimal zensical.toml project and return the config path."""
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "index.md").write_text("# Hello\n", encoding="utf-8")

    config = f'[project]\nsite_name = "Test"\n{toml_extra}\n'
    config_path = tmp_path / "zensical.toml"
    config_path.write_text(config, encoding="utf-8")

    return config_path


def _make_yml_project(
    tmp_path: Path,
    *,
    yml_extra: str = "",
) -> Path:
    """Scaffold a minimal mkdocs.yml project and return the config path."""
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "index.md").write_text("# Hello\n", encoding="utf-8")

    lines = [
        'site_name: "Test"',
        "theme:",
        "  name: material",
    ]
    if yml_extra:
        lines.append(yml_extra)
    config_path = tmp_path / "mkdocs.yml"
    config_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    return config_path


def _make_custom_dir(
    tmp_path: Path,
    *,
    mkdocs_theme_yml: str | None = None,
) -> Path:
    """Create an `overrides/` subdirectory for use as `custom_dir`."""
    custom = tmp_path / "overrides"
    custom.mkdir()
    if mkdocs_theme_yml is not None:
        (custom / "mkdocs_theme.yml").write_text(
            mkdocs_theme_yml, encoding="utf-8"
        )
    return custom


def test_symlinked_config_anchors_relative_paths_to_its_target(
    tmp_path: Path,
) -> None:
    """Python and Rust resolve project roots from the same config path."""
    project = tmp_path / "project"
    project.mkdir()
    config = _make_yml_project(project)
    alias_dir = tmp_path / "alias"
    alias_dir.mkdir()
    alias = alias_dir / "mkdocs.yml"
    try:
        alias.symlink_to(config)
    except OSError as error:
        pytest.skip(f"symbolic links unavailable: {error}")

    _build(alias)

    assert (project / "site" / "index.html").is_file()
    assert not (alias_dir / "site").exists()


def test_navigation_title_precedes_metadata_and_heading(tmp_path: Path) -> None:
    """Configured titles have the same highest precedence as in MkDocs."""
    config = _make_yml_project(
        tmp_path,
        yml_extra=(
            "  custom_dir: overrides\nnav:\n  - Configured title: index.md"
        ),
    )
    (tmp_path / "docs" / "index.md").write_text(
        "---\ntitle: Metadata title\n---\n\n# Heading title\n",
        encoding="utf-8",
    )
    custom = _make_custom_dir(tmp_path)
    (custom / "main.html").write_text("{{ page.title }}", encoding="utf-8")

    _build(config)

    assert (tmp_path / "site" / "index.html").read_text() == "Configured title"


@pytest.mark.parametrize(
    ("navigation", "expected"),
    [
        ("  - index.md\n  - Later: index.md", "Hello"),
        ("  - First: index.md\n  - Second: index.md", "First"),
    ],
)
def test_first_navigation_occurrence_owns_page_title(
    tmp_path: Path, navigation: str, expected: str
) -> None:
    """Duplicate pages retain the title assigned by their first occurrence."""
    config = _make_yml_project(
        tmp_path,
        yml_extra=(f"  custom_dir: overrides\nnav:\n{navigation}"),
    )
    custom = _make_custom_dir(tmp_path)
    (custom / "main.html").write_text("{{ page.title }}", encoding="utf-8")

    _build(config)

    assert (tmp_path / "site" / "index.html").read_text() == expected


def test_only_root_index_page_becomes_navigation_homepage(
    tmp_path: Path,
) -> None:
    """Nested index paths and similarly named links are never the homepage."""
    config = _make_yml_project(
        tmp_path,
        yml_extra=(
            "  custom_dir: overrides\n"
            "nav:\n"
            "  - Guide: guide/index.md\n"
            "  - Website: https://example.com/index.md"
        ),
    )
    guide = tmp_path / "docs" / "guide"
    guide.mkdir()
    (guide / "index.md").write_text("# Guide\n", encoding="utf-8")
    custom = _make_custom_dir(tmp_path)
    (custom / "main.html").write_text(
        "{{ nav.homepage.title if nav.homepage else 'none' }}",
        encoding="utf-8",
    )

    _build(config)

    assert (tmp_path / "site" / "index.html").read_text() == "Hello"


# ---------------------------------------------------------------------------
# Theme loading: both zensical.toml and mkdocs.yml
# ---------------------------------------------------------------------------


class TestThemeLoadingToml:
    """Theme resolution via zensical.toml (Rust TOML path)."""

    def test_default_theme_build_succeeds(self, tmp_path: Path) -> None:
        """No theme.name and no custom_dir -> builtin theme used, build OK."""
        config_path = _make_toml_project(tmp_path)
        _build(config_path)  # must not raise

    def test_unknown_theme_name_raises(self, tmp_path: Path) -> None:
        """theme.name set to an uninstalled theme -> config error raised."""
        config_path = _make_toml_project(
            tmp_path,
            toml_extra='[project.theme]\nname = "definitely-not-installed"',
        )
        with pytest.raises(ConfigurationError):
            _build(config_path)

    def test_custom_dir_no_mkdocs_theme_yml_build_succeeds(
        self, tmp_path: Path
    ) -> None:
        """custom_dir, no mkdocs_theme.yml -> fallback to builtin, build OK."""
        _make_custom_dir(tmp_path)
        config_path = _make_toml_project(
            tmp_path,
            toml_extra='[project.theme]\ncustom_dir = "overrides"\n',
        )
        _build(config_path)  # must not raise

    def test_custom_dir_mkdocs_theme_yml_no_extends_build_succeeds(
        self, tmp_path: Path
    ) -> None:
        """custom_dir, mkdocs_theme.yml, no extends -> fallback to builtin."""
        _make_custom_dir(tmp_path, mkdocs_theme_yml="name: my-overrides\n")
        config_path = _make_toml_project(
            tmp_path,
            toml_extra='[project.theme]\ncustom_dir = "overrides"\n',
        )
        _build(config_path)  # must not raise

    def test_custom_dir_extends_material_build_succeeds(
        self, tmp_path: Path
    ) -> None:
        """custom_dir, extends: material -> follows chain through builtin."""
        _make_custom_dir(tmp_path, mkdocs_theme_yml="extends: material\n")
        config_path = _make_toml_project(
            tmp_path,
            toml_extra='[project.theme]\ncustom_dir = "overrides"\n',
        )
        _build(config_path)  # must not raise

    def test_custom_dir_extends_zensical_build_succeeds(
        self, tmp_path: Path
    ) -> None:
        """custom_dir, extends: zensical -> same builtin chain as material."""
        _make_custom_dir(tmp_path, mkdocs_theme_yml="extends: zensical\n")
        config_path = _make_toml_project(
            tmp_path,
            toml_extra='[project.theme]\ncustom_dir = "overrides"\n',
        )
        _build(config_path)  # must not raise

    def test_custom_dir_extends_unknown_theme_raises(
        self, tmp_path: Path
    ) -> None:
        """custom_dir extends unknown theme -> ConfigurationError raised."""
        _make_custom_dir(
            tmp_path,
            mkdocs_theme_yml="extends: definitely-not-installed-xyzzy\n",
        )
        config_path = _make_toml_project(
            tmp_path,
            toml_extra='[project.theme]\ncustom_dir = "overrides"\n',
        )
        with pytest.raises(ConfigurationError):
            _build(config_path)

    def test_name_set_custom_dir_no_extends_name_used_as_base(
        self, tmp_path: Path
    ) -> None:
        """theme.name + custom_dir, no extends -> name used as base, succeeds.

        Contrast with test_custom_dir_extends_*: when the custom_dir has an
        explicit extends the name is ignored; when it doesn't, the name
        determines the base.
        """
        _make_custom_dir(tmp_path)
        config_path = _make_toml_project(
            tmp_path,
            toml_extra=(
                '[project.theme]\nname = "material"\ncustom_dir = "overrides"\n'
            ),
        )
        _build(config_path)  # must not raise

    def test_name_set_custom_dir_extends_overrides_name(
        self, tmp_path: Path
    ) -> None:
        """theme.name + custom_dir with extends -> chain followed, name ignored.

        The build must succeed regardless of what name is, because the chain
        is driven entirely by the extends declaration in mkdocs_theme.yml.
        """
        _make_custom_dir(tmp_path, mkdocs_theme_yml="extends: zensical\n")
        config_path = _make_toml_project(
            tmp_path,
            toml_extra=(
                "[project.theme]\n"
                # name would point to the same builtin anyway, but the key
                # assertion is that the extends chain is what is followed.
                'name = "material"\n'
                'custom_dir = "overrides"\n'
            ),
        )
        _build(config_path)  # must not raise

    def test_custom_dir_extends_installed_theme_chain_applies_palette(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """custom_dir -> installed theme -> preserve inherited palette.

        Chain under test:

        - overrides/mkdocs_theme.yml: extends: demo-installed-theme
        - demo-installed-theme/mkdocs_theme.yml:
            - extends: zensical
            - palette:
                - scheme: slate

        The final rendered config must include the inherited `slate` scheme.
        """
        installed_theme_name = "demo-installed-theme"

        # custom_dir extends an installed theme by name
        _make_custom_dir(
            tmp_path,
            mkdocs_theme_yml=f"extends: {installed_theme_name}\n",
        )

        # fake installed theme extends zensical and sets a slate palette
        installed_theme_dir = tmp_path / "demo_theme"
        installed_theme_dir.mkdir()
        (installed_theme_dir / "mkdocs_theme.yml").write_text(
            "extends: zensical\npalette:\n- scheme: slate\n",
            encoding="utf-8",
        )

        original_get_theme_dir = cfg_module.get_theme_dir

        def fake_get_theme_dir(name: str) -> str:
            if name == installed_theme_name:
                return str(installed_theme_dir)
            return original_get_theme_dir(name)

        monkeypatch.setattr(cfg_module, "get_theme_dir", fake_get_theme_dir)

        config_path = _make_toml_project(
            tmp_path,
            toml_extra='[project.theme]\ncustom_dir = "overrides"\n',
        )
        _build(config_path)

        html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        assert 'data-md-color-scheme="slate"' in html


class TestThemeLoadingYml:
    """Theme resolution via mkdocs.yml (Python/YAML path)."""

    def test_default_theme_build_succeeds(self, tmp_path: Path) -> None:
        """theme.name = material (explicit) -> builtin theme, build OK."""
        config_path = _make_yml_project(tmp_path)
        _build(config_path)  # must not raise

    def test_unknown_theme_name_raises(self, tmp_path: Path) -> None:
        """theme.name set to an uninstalled theme -> config error raised."""
        config_path = _make_yml_project(
            tmp_path,
            yml_extra="theme:\n  name: definitely-not-installed-xyzzy",
        )
        with pytest.raises(ConfigurationError):
            _build(config_path)

    def test_custom_dir_no_mkdocs_theme_yml_build_succeeds(
        self, tmp_path: Path
    ) -> None:
        """custom_dir, no mkdocs_theme.yml -> fallback to builtin, build OK."""
        _make_custom_dir(tmp_path)
        config_path = _make_yml_project(
            tmp_path,
            yml_extra="theme:\n  name: material\n  custom_dir: overrides",
        )
        _build(config_path)  # must not raise

    def test_custom_dir_extends_material_build_succeeds(
        self, tmp_path: Path
    ) -> None:
        """custom_dir, extends: material -> follows builtin chain, succeeds."""
        _make_custom_dir(tmp_path, mkdocs_theme_yml="extends: material\n")
        config_path = _make_yml_project(
            tmp_path,
            yml_extra="theme:\n  name: material\n  custom_dir: overrides",
        )
        _build(config_path)  # must not raise

    def test_custom_dir_extends_installed_theme_chain_applies_palette(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """custom_dir -> installed theme -> preserve inherited palette."""
        installed_theme_name = "demo-installed-theme"

        # custom_dir extends an installed theme by name
        _make_custom_dir(
            tmp_path,
            mkdocs_theme_yml=f"extends: {installed_theme_name}\n",
        )

        # fake installed theme extends zensical and sets a slate palette
        installed_theme_dir = tmp_path / "demo_theme"
        installed_theme_dir.mkdir()
        (installed_theme_dir / "mkdocs_theme.yml").write_text(
            "extends: zensical\npalette:\n- scheme: slate\n",
            encoding="utf-8",
        )

        original_get_theme_dir = cfg_module.get_theme_dir

        def fake_get_theme_dir(name: str) -> str:
            if name == installed_theme_name:
                return str(installed_theme_dir)
            return original_get_theme_dir(name)

        monkeypatch.setattr(cfg_module, "get_theme_dir", fake_get_theme_dir)

        config_path = _make_yml_project(
            tmp_path,
            yml_extra="theme:\n  name: material\n  custom_dir: overrides",
        )
        _build(config_path)

        html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        assert 'data-md-color-scheme="slate"' in html

    def test_custom_dir_extends_unknown_theme_raises(
        self, tmp_path: Path
    ) -> None:
        """custom_dir extends unknown theme -> ConfigurationError raised."""
        _make_custom_dir(
            tmp_path,
            mkdocs_theme_yml="extends: definitely-not-installed-xyzzy\n",
        )
        config_path = _make_yml_project(
            tmp_path,
            yml_extra="theme:\n  name: material\n  custom_dir: overrides",
        )
        with pytest.raises(ConfigurationError):
            _build(config_path)
