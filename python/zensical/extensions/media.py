# Copyright (c) 2025-2026 Zensical and contributors
# Media behavior adapted from mkdocs-video, copyright (c) 2023 Mikalai Lisitsa,
# and mkdocs-audio, copyright (c) 2024 Jean-François Cartier.
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

from dataclasses import dataclass, field
from html import unescape
from html.parser import HTMLParser
from typing import TYPE_CHECKING, Any
from xml.etree.ElementTree import Element, tostring

from markdown import Extension, Markdown
from markdown.postprocessors import Postprocessor
from markdown.treeprocessors import Treeprocessor, UnescapeTreeprocessor

if TYPE_CHECKING:
    from collections.abc import Mapping


@dataclass
class VideoConfig:
    """Settings compatible with mkdocs-video."""

    enabled: bool = True
    mark: str = "type:video"
    is_video: bool = False
    video_type: str = "mp4"
    video_muted: bool = False
    video_loop: bool = False
    video_controls: bool = True
    video_autoplay: bool = False
    css_style: dict[str, Any] = field(
        default_factory=lambda: {
            "position": "relative",
            "width": "100%",
            "height": "22.172vw",
        }
    )


@dataclass
class AudioConfig:
    """Settings compatible with mkdocs-audio."""

    enabled: bool = True
    mark: str = "type:audio"
    audio_type: str = "mp3"
    audio_loop: bool = False
    audio_controls: bool = True
    audio_autoplay: bool = False
    css_style: dict[str, Any] = field(default_factory=lambda: {"width": "100%"})


@dataclass
class MediaConfig:
    """Shared settings for the media processors."""

    video: VideoConfig
    audio: AudioConfig
    order: tuple[str, str]


def _media_type(kind: str, subtype: str) -> str:
    """Validate the upstream MIME subtype syntax."""
    subtype = subtype.lower().strip()
    if any(char in subtype for char in (" ", "/")):
        raise ValueError(f"Unsupported {kind} type")
    return f"{kind}/{subtype}"


def _style(values: Mapping[str, Any]) -> str:
    return ";".join(f"{key}:{value}" for key, value in values.items())


def _raw_source(source: Element) -> str:
    """Serialize the HTML void element absent from Markdown's tag catalog."""
    return tostring(source, encoding="unicode", method="html").removesuffix(
        "</source>"
    )


def _replace_image(
    attrs: Mapping[str, str], config: MediaConfig
) -> Element | None:
    """Build a media element from decoded image attributes."""
    src = attrs.get("src")
    if not src:
        return None

    for kind in config.order:
        settings = config.video if kind == "video" else config.audio
        if settings.enabled and attrs.get("alt") == settings.mark:
            break
    else:
        return None

    global_config = "disable-global-config" not in attrs
    tag = "iframe" if kind == "video" and not config.video.is_video else kind
    media = Element(tag)

    if tag == "iframe":
        media.set("src", src)
    else:
        source = Element("source", {"src": src})
        subtype = (
            config.video.video_type
            if kind == "video"
            else config.audio.audio_type
        )
        source.set("type", _media_type(kind, subtype))
        media.append(source)

    # Audio's controls setting is independent of disable-global-config. The
    # upstream plugin always emits controls, even when configured false; omit
    # it for false so browsers actually hide the controls.
    if kind == "audio" and config.audio.audio_controls:
        media.set("controls", "")

    if global_config:
        media.set("style", _style(settings.css_style))
        if kind == "video":
            if tag == "iframe":
                media.set("frameborder", "0")
                media.set("allowfullscreen", "")
            else:
                for enabled, name in (
                    (config.video.video_loop, "loop"),
                    (config.video.video_muted, "muted"),
                    (config.video.video_controls, "controls"),
                    (config.video.video_autoplay, "autoplay"),
                ):
                    if enabled:
                        media.set(name, "")
        else:
            if config.audio.audio_loop:
                media.set("loop", "")
            if config.audio.audio_autoplay:
                media.set("autoplay", "")

    # Attribute lists have already updated the image, and take precedence over
    # the global defaults. The image source belongs to iframe or source only.
    for name, value in attrs.items():
        if name not in {"src", "disable-global-config"}:
            media.set(name, value)

    wrapper = Element("div", {"class": f"{kind}-container"})
    wrapper.append(media)
    return wrapper


class MediaTreeprocessor(Treeprocessor):
    """Transform Markdown image nodes after attr_list has run."""

    name = "media"

    def __init__(self, md: Markdown, config: MediaConfig):
        super().__init__(md)
        self.config = config
        self._unescape = UnescapeTreeprocessor(md).unescape

    def run(self, root: Element) -> None:
        for parent in root.iter():
            for index, img in enumerate(list(parent)):
                if img.tag != "img":
                    continue
                # Markdown keeps entities in image attributes until its
                # serialization step; HTMLParser has already decoded them.
                attrs = {
                    name: unescape(value) for name, value in img.attrib.items()
                }
                replacement = _replace_image(attrs, self.config)
                if replacement is not None:
                    # Python Markdown does not serialize <source> as a void
                    # element. Stash its HTML so raw_html restores the proper
                    # tag after the URL postprocessor has rewritten its src.
                    source = replacement.find(".//source")
                    if source is not None:
                        media = replacement[0]
                        media.remove(source)
                        source.set("src", self._unescape(source.get("src", "")))
                        media.text = self.md.htmlStash.store(
                            _raw_source(source)
                        )
                    replacement.tail = img.tail
                    parent[index] = replacement


class _RawMediaParser(HTMLParser):
    """Find image tag spans without changing unrelated raw HTML."""

    def __init__(self, source: str, config: MediaConfig):
        super().__init__(convert_charrefs=False)
        self.source = source
        self.config = config
        self.replacements: list[tuple[int, int, str]] = []
        self.line_starts = [0]
        self.line_starts.extend(
            index + 1 for index, char in enumerate(source) if char == "\n"
        )

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        if tag != "img":
            return
        decoded = {name: value or "" for name, value in attrs}
        replacement = _replace_image(decoded, self.config)
        if replacement is not None:
            line, column = self.getpos()
            start = self.line_starts[line - 1] + column
            raw = self.get_starttag_text()
            if raw is not None:
                html = tostring(replacement, encoding="unicode", method="html")
                html = html.replace("</source>", "")
                self.replacements.append(
                    (
                        start,
                        start + len(raw),
                        html,
                    )
                )

    def handle_startendtag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        self.handle_starttag(tag, attrs)

    def convert(self) -> str:
        self.feed(self.source)
        if not self.replacements:
            return self.source
        parts: list[str] = []
        cursor = 0
        for start, end, replacement in self.replacements:
            parts.extend((self.source[cursor:start], replacement))
            cursor = end
        parts.append(self.source[cursor:])
        return "".join(parts)


class MediaPostprocessor(Postprocessor):
    """Transform images in Python Markdown's raw HTML stash."""

    name = "media"

    def __init__(self, md: Markdown, config: MediaConfig):
        super().__init__(md)
        self.config = config
        self._cursor = 0
        self._blocks = md.htmlStash.rawHtmlBlocks

    def run(self, text: str) -> str:
        blocks = self.md.htmlStash.rawHtmlBlocks
        if blocks is not self._blocks:
            self._blocks = blocks
            self._cursor = 0
        while self._cursor < len(blocks):
            block = blocks[self._cursor]
            if isinstance(block, str) and "<img" in block.lower():
                parser = _RawMediaParser(block, self.config)
                blocks[self._cursor] = parser.convert()
            self._cursor += 1
        return text


class MediaFinalPostprocessor(Postprocessor):
    """Transform marked images emitted by later Markdown postprocessors."""

    name = "media_final"

    def __init__(self, md: Markdown, config: MediaConfig):
        super().__init__(md)
        self.config = config

    def run(self, text: str) -> str:
        if "<img" not in text.lower():
            return text
        return _RawMediaParser(text, self.config).convert()


class MediaExtension(Extension):
    """Embed video and audio using marked Markdown images."""

    name = "zensical.extensions.media"

    def __init__(self, **kwargs: Any) -> None:
        self.enabled = kwargs.pop("enabled", True)
        video = kwargs.pop("video", {})
        audio = kwargs.pop("audio", {})
        order = kwargs.pop("order", ("video", "audio"))
        if kwargs:
            raise ValueError(
                f"Unknown media options: {', '.join(sorted(kwargs))}"
            )
        if not isinstance(video, dict) or not isinstance(audio, dict):
            raise TypeError("Media video and audio settings must be mappings")
        if not isinstance(order, (list, tuple)) or tuple(order) not in (
            ("video", "audio"),
            ("audio", "video"),
        ):
            raise ValueError("Media order must contain video and audio once")
        self.media_config = MediaConfig(
            VideoConfig(**video), AudioConfig(**audio), tuple(order)
        )

    def extendMarkdown(self, md: Markdown) -> None:
        if not self.enabled or not (
            self.media_config.video.enabled or self.media_config.audio.enabled
        ):
            return
        md.registerExtension(self)

        # attr_list runs at 8, glightbox at 7, and URL rewriting at 0.
        tree = MediaTreeprocessor(md, self.media_config)
        md.treeprocessors.register(tree, tree.name, 7.5)

        # Run before glightbox and URL rewriting (31), then raw_html (30).
        post = MediaPostprocessor(md, self.media_config)
        md.postprocessors.register(post, post.name, 32)

        # The MkDocs plugins run on final page HTML. Catch images emitted by
        # other postprocessors after the tree and raw HTML stash were visited.
        final = MediaFinalPostprocessor(md, self.media_config)
        md.postprocessors.register(final, final.name, 19)


def makeExtension(**kwargs: Any) -> MediaExtension:
    """Register the media Markdown extension."""
    return MediaExtension(**kwargs)
