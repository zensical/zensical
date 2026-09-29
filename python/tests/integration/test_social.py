# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

"""Integration tests for native MkDocs Material social compatibility."""

from __future__ import annotations

import struct
from typing import TYPE_CHECKING, Any

import pytest

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def _png_size(path: Path) -> tuple[int, int]:
    """Read PNG dimensions without adding an imaging test dependency."""
    data = path.read_bytes()
    assert data.startswith(b"\x89PNG\r\n\x1a\n")
    return struct.unpack(">II", data[16:24])


def _write_project(root: Path) -> Path:
    """Create a social project whose layout requires no network access."""
    docs = root / "docs"
    layouts = root / "layouts"
    docs.mkdir()
    (docs / "guide").mkdir()
    layouts.mkdir()
    (docs / "index.md").write_text(
        "---\ntitle: 'A social & card'\n---\n# Home\n",
        encoding="utf-8",
    )
    (docs / "guide" / "index.md").write_text(
        "---\nsocial:\n  cards: false\n---\n# Guide\n",
        encoding="utf-8",
    )
    (layouts / "plain.yml").write_text(
        """\
tags:
  og:type: website
  og:title: "{{ page.title }}"
  og:image: "{{ image.url }}"
  og:image:width: "{{ image.width }}"
  x:layout: "{{ layout.label }}"
size: { width: 320, height: 168 }
layers:
  - background: { color: "#123456" }
""",
        encoding="utf-8",
    )
    config = root / "mkdocs.yml"
    config.write_text(
        """\
site_name: Social
site_url: https://example.com/docs
theme:
  name: material
plugins:
  - material/social:
      cache: false
      cards_layout: plain
      cards_include: ['*.md']
      cards_layout_options:
        label: measured
""",
        encoding="utf-8",
    )
    return config


def test_generates_custom_card_and_injects_metadata(tmp_path: Path) -> None:
    """YAML, MiniJinja, raster output and HTML injection work together."""
    config = _write_project(tmp_path)

    zensical.build(str(config), _BUILD_OPTIONS)

    card = tmp_path / "site" / "assets" / "images" / "social" / "index.png"
    assert _png_size(card) == (320, 168)

    html = (tmp_path / "site" / "index.html").read_text()
    assert '<meta property="og:type" content="website" />' in html
    assert '<meta property="og:title" content="A social &amp; card" />' in html
    assert (
        '<meta property="og:image" '
        'content="https://example.com/docs/assets/images/social/index.png" />'
        in html
    )
    assert '<meta property="og:image:width" content="320" />' in html
    assert '<meta property="x:layout" content="measured" />' in html
    assert html.index('<meta property="og:type"') < html.index("</head>")

    assert not (
        tmp_path
        / "site"
        / "assets"
        / "images"
        / "social"
        / "guide"
        / "index.png"
    ).exists()


def test_material_namespace_and_multiple_instances_are_preserved(
    tmp_path: Path,
) -> None:
    """Canonical and namespaced aliases remain ordered plugin instances."""
    config = _write_project(tmp_path)
    config.write_text(
        """\
site_name: Social
site_url: https://example.com/docs
theme:
  name: material
plugins:
  - material/social:
      cache: false
      cards_dir: assets/cards/first
      cards_layout: plain
  - social/second:
      cache: false
      cards_dir: assets/cards/second
      cards_layout: plain
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    first = tmp_path / "site" / "assets" / "cards" / "first" / "index.png"
    second = tmp_path / "site" / "assets" / "cards" / "second" / "index.png"
    assert first.is_file()
    assert second.is_file()


def test_later_instance_owns_a_shared_card_path(tmp_path: Path) -> None:
    """Output collisions follow MkDocs plugin ordering deterministically."""
    config = _write_project(tmp_path)
    (tmp_path / "layouts" / "wide.yml").write_text(
        (tmp_path / "layouts" / "plain.yml")
        .read_text()
        .replace("width: 320, height: 168", "width: 640, height: 320"),
        encoding="utf-8",
    )
    config.write_text(
        """\
site_name: Social
site_url: https://example.com
theme: { name: material }
plugins:
  - social: { cache: false, cards_layout: plain }
  - social/last: { cache: false, cards_layout: wide }
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    card = tmp_path / "site" / "assets" / "images" / "social" / "index.png"
    assert _png_size(card) == (640, 320)


def test_without_site_url_generates_but_does_not_link_card(
    tmp_path: Path,
) -> None:
    """A missing site URL suppresses metadata without suppressing output."""
    config = _write_project(tmp_path)
    config.write_text(
        config.read_text().replace("site_url: https://example.com/docs\n", "")
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (
        tmp_path / "site" / "assets" / "images" / "social" / "index.png"
    ).is_file()
    assert (
        '<meta property="og:image"'
        not in (tmp_path / "site" / "index.html").read_text()
    )


def test_cache_tracks_local_image_contents(tmp_path: Path) -> None:
    """A changed layout dependency invalidates the persistent card cache."""
    config = _write_project(tmp_path)
    layout = tmp_path / "layouts" / "plain.yml"
    layout.write_text(
        layout.read_text().replace(
            'background: { color: "#123456" }',
            'background: { image: "{{ config.docs_dir }}/background.svg" }',
        ),
        encoding="utf-8",
    )
    background = tmp_path / "docs" / "background.svg"
    background.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">'
        '<rect width="10" height="10" fill="red"/></svg>',
        encoding="utf-8",
    )
    config.write_text(config.read_text().replace("      cache: false\n", ""))

    zensical.build(str(config), _BUILD_OPTIONS)
    card = tmp_path / "site" / "assets" / "images" / "social" / "index.png"
    before = card.read_bytes()

    background.write_text(
        background.read_text().replace('fill="red"', 'fill="blue"'),
        encoding="utf-8",
    )
    zensical.build(str(config), _BUILD_OPTIONS)

    assert card.read_bytes() != before
    cached = (tmp_path / ".cache/plugin/social/cards").glob("*.png")
    assert len(list(cached)) == 2


def test_unrelated_images_do_not_invalidate_cached_cards(
    tmp_path: Path,
) -> None:
    """The invalidation signal does not become part of the card cache key."""
    config = _write_project(tmp_path)
    config.write_text(
        config.read_text().replace("      cache: false\n", "")
    )

    zensical.build(str(config), _BUILD_OPTIONS)
    cache = tmp_path / ".cache/plugin/social/cards"
    assert len(list(cache.glob("*.png"))) == 1

    (tmp_path / "docs" / "unused.svg").write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>',
        encoding="utf-8",
    )
    zensical.build(str(config), _BUILD_OPTIONS)

    assert len(list(cache.glob("*.png"))) == 1


def test_concurrent_identical_cards_do_not_share_temporary_files(
    tmp_path: Path,
) -> None:
    """Equal card digests remain safe across concurrent page jobs."""
    config = _write_project(tmp_path)
    config.write_text(
        config.read_text().replace(
            "      cache: false\n",
            "      cache: false\n      concurrency: 8\n",
        )
    )
    for index in range(8):
        (tmp_path / "docs" / f"page-{index}.md").write_text(
            f"# Page {index}\n",
            encoding="utf-8",
        )

    zensical.build(str(config), _BUILD_OPTIONS)

    directory = tmp_path / "site/assets/images/social"
    assert all(
        (directory / f"page-{index}.png").is_file()
        for index in range(8)
    )


def test_supports_bundled_image_only_layout(tmp_path: Path) -> None:
    """All layouts shipped by upstream are available without Python imaging."""
    config = _write_project(tmp_path)
    background = tmp_path / "docs" / "background.svg"
    background.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1">'
        '<rect width="2" height="1" fill="orange"/></svg>',
        encoding="utf-8",
    )
    config.write_text(
        config.read_text()
        .replace(
            "      cards_layout: plain\n",
            "      cards_layout: default/only/image\n",
        )
        .replace(
            "      cards_layout_options:\n        label: measured\n",
                "      cards_layout_options:\n"
                "        background_image: docs/background.svg\n",
        )
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    card = tmp_path / "site/assets/images/social/index.png"
    assert _png_size(card) == (1200, 630)
    html = (tmp_path / "site/index.html").read_text()
    assert '<meta property="og:title" content="A social &amp; card" />' in html


def test_root_readme_is_a_homepage_in_layout_context(tmp_path: Path) -> None:
    """MkDocs treats a root README exactly like a root index page."""
    config = _write_project(tmp_path)
    (tmp_path / "docs/index.md").rename(tmp_path / "docs/README.md")
    layout = tmp_path / "layouts/plain.yml"
    layout.write_text(
        layout.read_text().replace(
            '  x:layout: "{{ layout.label }}"\n',
            '  x:layout: "{{ layout.label }}"\n'
            '  x:homepage: "{{ page.is_homepage }}"\n',
        ),
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    html = (tmp_path / "site/index.html").read_text()
    assert '<meta property="x:homepage" content="true" />' in html


def test_honors_documented_log_levels_and_strict_mode(tmp_path: Path) -> None:
    """Ignored errors stay quiet, while warnings fail strict builds."""
    config = _write_project(tmp_path)
    layout = tmp_path / "layouts/plain.yml"
    layout.write_text(
        layout.read_text().replace(
            'background: { color: "#123456" }',
            'background: { image: "missing.png" }',
        ),
        encoding="utf-8",
    )
    config.write_text(
        config.read_text().replace(
            "      cache: false\n",
            "      cache: false\n      log_level: ignore\n",
        )
    )

    zensical.build(str(config), _BUILD_OPTIONS)
    assert not (
        tmp_path / "site/assets/images/social/index.png"
    ).exists()

    config.write_text(
        config.read_text().replace("log_level: ignore", "log_level: warn")
    )
    with pytest.raises(RuntimeError, match="strict flag"):
        zensical.build(
            str(config), {"clean": False, "strict": True}
        )


def test_rejects_unknown_social_configuration(tmp_path: Path) -> None:
    """Rust validation reports the precise plugin option path."""
    config = _write_project(tmp_path)
    config.write_text(
        config.read_text().replace(
            "      cache: false\n", "      unknown: true\n"
        )
    )

    with pytest.raises(ValueError, match=r"plugins\.social\.unknown"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_rejects_invalid_page_overrides_before_error_logging(
    tmp_path: Path,
) -> None:
    """Page configuration errors remain fatal when render errors are logged."""
    config = _write_project(tmp_path)
    index = tmp_path / "docs/index.md"
    index.write_text(
        "---\nsocial:\n  cards_layout_options: invalid\n---\n# Home\n",
        encoding="utf-8",
    )

    with pytest.raises(RuntimeError, match="cards_layout_options"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_warns_for_deprecated_options(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Accepted legacy settings point users to their layout replacements."""
    config = _write_project(tmp_path)
    config.write_text(
        config.read_text().replace(
            "      cache: false\n",
            "      cache: false\n"
            "      cards_color: red\n"
            "      cards_font: Roboto\n",
        )
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    stderr = capfd.readouterr().err
    assert "'cards_color' option" in stderr
    assert "'cards_font' option" in stderr
