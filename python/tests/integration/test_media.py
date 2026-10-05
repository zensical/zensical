# Copyright (c) 2025-2026 Zensical and contributors
#
# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to
# deal in the Software without restriction, including without limitation the
# rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
# sell copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
# FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
# IN THE SOFTWARE.

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTS: dict[str, Any] = {"clean": False, "strict": False}


def test_both_legacy_plugins_render_through_media_extension(
    tmp_path: Path,
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Media\n\n"
        "![type:video](https://example.com/movie.ogg)\n\n"
        "![type:audio](sound.mp3)\n",
        encoding="utf-8",
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        "site_name: Media\n"
        "plugins:\n"
        "  - mkdocs-video:\n"
        "      is_video: true\n"
        "      video_type: ogg\n"
        "  - mkdocs-audio:\n"
        "      audio_controls: false\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    page = BeautifulSoup(html, "html.parser")
    video = page.select_one(".video-container video")
    audio = page.select_one(".audio-container audio")
    assert video is not None
    assert audio is not None
    assert video.source is not None
    assert audio.source is not None
    assert video.source["src"] == "https://example.com/movie.ogg"
    assert video.source["type"] == "video/ogg"
    assert audio.source["src"] == "sound.mp3"
    assert not audio.has_attr("controls")


def test_native_media_config_enables_audio_independently(
    tmp_path: Path,
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Media\n\n![type:video](movie.mp4)\n\n![type:audio](sound.mp3)\n",
        encoding="utf-8",
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        "[project]\n"
        'site_name = "Media"\n'
        '[project.markdown_extensions."zensical.extensions.media".video]\n'
        "enabled = false\n"
        '[project.markdown_extensions."zensical.extensions.media".audio]\n'
        "enabled = true\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    page = BeautifulSoup(html, "html.parser")
    assert page.select_one(".audio-container audio") is not None
    assert page.select_one(".video-container") is None
    assert page.select_one('img[alt="type:video"]') is not None


def test_shared_marker_uses_legacy_plugin_order(tmp_path: Path) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Media\n\n![shared](sound.mp3)\n", encoding="utf-8"
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        "site_name: Media\n"
        "plugins:\n"
        "  - mkdocs-audio:\n"
        "      mark: shared\n"
        "  - mkdocs-video:\n"
        "      mark: shared\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    page = BeautifulSoup(html, "html.parser")
    assert page.select_one(".audio-container audio") is not None
    assert page.select_one(".video-container") is None
