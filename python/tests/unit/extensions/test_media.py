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

import pytest
from markdown import Markdown
from markdown.postprocessors import Postprocessor

from tests.unit.extensions.conftest import soup
from zensical.extensions.glightbox import GlightboxExtension
from zensical.extensions.links import LinksExtension
from zensical.extensions.media import MediaExtension


def _markdown(**config: object) -> Markdown:
    return Markdown(extensions=["attr_list", MediaExtension(**config)])


def test_default_video_and_audio_embeds() -> None:
    md = _markdown()
    page = soup(
        md.convert(
            "![type:video](https://example.com/embed/1)\n\n"
            "![type:audio](sound.mp3)"
        )
    )

    iframe = page.select_one(".video-container > iframe")
    assert iframe is not None
    assert iframe["src"] == "https://example.com/embed/1"
    assert iframe["style"] == "position:relative;width:100%;height:22.172vw"
    assert iframe["frameborder"] == "0"
    assert iframe.has_attr("allowfullscreen")

    audio = page.select_one(".audio-container > audio")
    assert audio is not None
    assert audio.has_attr("controls")
    assert audio["style"] == "width:100%"
    source = audio.find("source")
    assert source is not None
    assert source["src"] == "sound.mp3"
    assert source["type"] == "audio/mp3"


@pytest.mark.parametrize("kind", ["video", "audio"])
def test_media_can_be_enabled_independently(kind: str) -> None:
    config = {
        "video": {"enabled": kind == "video"},
        "audio": {"enabled": kind == "audio"},
    }
    page = soup(
        _markdown(**config).convert(
            "![type:video](movie.mp4)\n\n![type:audio](sound.mp3)"
        )
    )

    assert (page.select_one(".video-container") is not None) == (
        kind == "video"
    )
    assert (page.select_one(".audio-container") is not None) == (
        kind == "audio"
    )
    assert len(page.find_all("img")) == 1


def test_video_element_options_and_attribute_overrides() -> None:
    md = _markdown(
        video={
            "is_video": True,
            "video_type": " OGG ",
            "video_muted": True,
            "video_loop": True,
            "video_controls": False,
            "video_autoplay": True,
            "css_style": {"width": "50%"},
        },
        audio={"enabled": False},
    )
    page = soup(
        md.convert(
            "![type:video](original.mp4)"
            "{: src='other.ogg' style='width: 80%' .demo}"
        )
    )

    video = page.select_one(".video-container > video.demo")
    assert video is not None
    assert video["style"] == "width: 80%"
    assert video.has_attr("muted")
    assert video.has_attr("loop")
    assert video.has_attr("autoplay")
    assert not video.has_attr("controls")
    assert not video.has_attr("src")
    assert video.source is not None
    assert video.source["src"] == "other.ogg"
    assert video.source["type"] == "video/ogg"


def test_audio_options_and_disabling_globals() -> None:
    md = _markdown(
        video={"enabled": False},
        audio={
            "mark": "sound",
            "audio_type": " OGG ",
            "audio_controls": False,
            "audio_loop": True,
            "audio_autoplay": True,
        },
    )
    page = soup(
        md.convert(
            "![sound](a.ogg)\n\n"
            "![sound](b.ogg){: disable-global-config style='width: 60%'}"
        )
    )

    first, second = page.select(".audio-container > audio")
    assert not first.has_attr("controls")
    assert first.has_attr("loop")
    assert first.has_attr("autoplay")
    assert first.source is not None
    assert first.source["type"] == "audio/ogg"
    assert second["style"] == "width: 60%"
    assert not second.has_attr("loop")
    assert not second.has_attr("autoplay")
    assert not second.has_attr("disable-global-config")


def test_raw_html_and_markdown_media_precede_image_and_url_processing() -> None:
    md = Markdown(
        extensions=[
            "attr_list",
            GlightboxExtension(),
            MediaExtension(),
            LinksExtension(path="guide/page.md", use_directory_urls=True),
        ]
    )
    html = md.convert(
        '<p><img alt="type:audio" src="sound.mp3"></p>\n\n'
        "![type:video](movie.mp4)\n\n![Picture](picture.png)"
    )
    page = soup(html)

    audio_source = page.select_one(".audio-container source")
    video_frame = page.select_one(".video-container iframe")
    ordinary_img = page.select_one("a.glightbox img")
    assert audio_source is not None
    assert video_frame is not None
    assert ordinary_img is not None
    assert audio_source["src"] == "../sound.mp3"
    assert video_frame["src"] == "../movie.mp4"
    assert len(page.select("a.glightbox")) == 1
    assert ordinary_img["src"] == "../picture.png"


def test_raw_html_preserves_unrelated_markup_and_inline_text() -> None:
    md = _markdown()
    html = md.convert(
        "<script>const example = "
        "\"<img alt='type:video' src='x'>\";"
        "</script>\n\n"
        '<p class="demo">before <img alt="type:video" src="a>b.mp4"> after</p>'
    )
    assert (
        "<script>const example = \"<img alt='type:video' src='x'>\";</script>"
        in html
    )
    assert 'class="demo"' in html
    assert "after</p>" in html
    frame = soup(html).select_one(".video-container iframe")
    assert frame is not None
    assert frame["src"] == "a>b.mp4"


def test_tree_preserves_text_after_inline_image() -> None:
    html = _markdown().convert("before ![type:video](movie.mp4) after")
    assert "after</p>" in html


def test_source_url_restores_markdown_escapes() -> None:
    html = _markdown().convert(r"![type:audio](a\_b.mp3?x=1&y=2)")
    source = soup(html).select_one(".audio-container source")
    assert source is not None
    assert source["src"] == "a_b.mp3?x=1&y=2"


def test_markdown_entities_match_media_markers_and_sources() -> None:
    html = _markdown().convert("![type&#58;audio](a&#46;mp3)")
    source = soup(html).select_one(".audio-container source")
    assert source is not None
    assert source["src"] == "a.mp3"


def test_raw_html_entities_are_decoded_only_once() -> None:
    html = _markdown().convert('<img alt="type:audio" src="a&amp;amp;b.mp3">')
    source = soup(html).select_one(".audio-container source")
    assert source is not None
    assert source["src"] == "a&amp;b.mp3"


def test_media_from_late_markdown_postprocessor() -> None:
    class LateImage(Postprocessor):
        def run(self, text: str) -> str:
            return text + '<p><img alt="type:audio" src="late.mp3"></p>'

    md = _markdown()
    md.postprocessors.register(LateImage(md), "late_image", 25)

    source = soup(md.convert("# Media")).select_one(".audio-container source")
    assert source is not None
    assert source["src"] == "late.mp3"


def test_markdown_instance_can_be_reset() -> None:
    md = _markdown()
    assert "audio-container" in md.convert('<img alt="type:audio" src="a.mp3">')
    md.reset()
    assert "audio-container" in md.convert('<img alt="type:audio" src="b.mp3">')
