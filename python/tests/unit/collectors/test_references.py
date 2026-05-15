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

from zensical.collectors import references
from zensical.collectors.references import (
    FootnoteDefinition,
    FootnoteReference,
    Link,
    LinkDefinition,
    LinkReference,
    Reference,
)

if TYPE_CHECKING:
    from zensical.utilities.span import Span


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


class TestLinks:
    """Tests for links."""

    @pytest.mark.parametrize(
        ("md", "expected_text", "expected_href"),
        [
            pytest.param(
                b"[text](href)",
                b"text",
                b"href",
                id="link",
            ),
            pytest.param(
                b'[text](href "Title")',
                b"text",
                b"href",
                id="link-with-title",
            ),
            pytest.param(
                b'[text](href more"Title")',
                b"text",
                b"href more",
                id="link-with-title, whitespace",
            ),
            pytest.param(
                b'[text](href more "Title")',
                b"text",
                b"href more",
                id="link-with-title, whitespace, trailing",
            ),
            pytest.param(
                b"[](href)",
                b"",
                b"href",
                id="link-with-empty-text",
            ),
            pytest.param(
                b"[text]()",
                b"text",
                b"",
                id="link-with-empty-href",
            ),
            pytest.param(
                b"[]()",
                b"",
                b"",
                id="link-with-empty-text-and-href",
            ),
            pytest.param(
                b"[text \\]](href)",
                b"text \\]",
                b"href",
                id="link-with-escaped-brackets-in-text",
            ),
            pytest.param(
                b"[text]((href))",
                b"text",
                b"(href)",
                id="link-with-parens-in-href",
            ),
            pytest.param(
                b"[text](href 'Title')",
                b"text",
                b"href",
                id="link-with-title, single quotes",
            ),
            pytest.param(
                b"[text](href more'Title')",
                b"text",
                b"href more",
                id="link-with-title, single quotes, whitespace",
            ),
            pytest.param(
                b"[text](href more 'Title')",
                b"text",
                b"href more",
                id="link-with-title, single quotes, whitespace, trailing",
            ),
        ],
    )
    def test_link(
        self, md: bytes, expected_text: bytes, expected_href: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == expected_text
        assert text(md, links[0].href) == expected_href

    @pytest.mark.parametrize(
        ("md", "expected_text", "expected_href"),
        [
            pytest.param(
                b"[text](href){}",
                b"text",
                b"href",
                id="link-with-attr",
            ),
            pytest.param(
                b"[text](href){ .class }",
                b"text",
                b"href",
                id="link-with-class-attr",
            ),
            pytest.param(
                b"[text](href){ #id }",
                b"text",
                b"href",
                id="link-with-id-attr",
            ),
            pytest.param(
                b'[text](href){ target="_blank" }',
                b"text",
                b"href",
                id="link-with-target-attr",
            ),
            pytest.param(
                b'[text](href){ .class #id target="_blank" }',
                b"text",
                b"href",
                id="link-with-multiple-attrs",
            ),
            pytest.param(
                b'[text](href) { target="_blank" }',
                b"text",
                b"href",
                id="link-with-spaced-attr, invalid",
            ),
        ],
    )
    def test_link_with_attr_list(
        self, md: bytes, expected_text: bytes, expected_href: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == expected_text
        assert text(md, links[0].href) == expected_href

    @pytest.mark.parametrize(
        ("md", "expected_href"),
        [
            pytest.param(
                b"[text](<href>)",
                b"href",
                id="link-with-angle-brackets",
            ),
            pytest.param(
                b'[text](<href> "title")',
                b"href",
                id="link-with-angle-brackets-and-title",
            ),
            pytest.param(
                b'[text](<href> more"Title")',
                b"<href> more",
                id="link-with-angle-brackets-and-title, whitespace",
            ),
            pytest.param(
                b'[text](<href> more "Title")',
                b"<href> more",
                id="link-with-angle-brackets-and-title, whitespace, trailing",
            ),
            pytest.param(
                b"[text](<>)",
                b"",
                id="link-with-empty-angle-brackets",
            ),
            pytest.param(
                b"[text](<href)",
                b"<href",
                id="link-with-missing-closing-bracket",
            ),
        ],
    )
    def test_link_with_angle_brackets(
        self, md: bytes, expected_href: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].href) == expected_href

    def test_link_text_with_newline(self) -> None:
        md = b"[text\nmore text](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == b"text\nmore text"
        assert text(md, links[0].href) == b"href"

    def test_link_href_with_newline(self) -> None:
        md = b"[text](hr\nef)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"hr\nef"

    # --- negative cases ---

    def test_no_link_escaped_brackets(self) -> None:
        md = b"\\[text\\](href)"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_link_escaped_opening_bracket(self) -> None:
        md = b"\\[text](href)"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_link_escaped_closing_bracket(self) -> None:
        md = b"[text\\](href)"
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestLinkReferences:
    """Tests for link references."""

    @pytest.mark.parametrize(
        ("md", "expected_text", "expected_id"),
        [
            pytest.param(
                b"[text][id]",
                b"text",
                b"id",
                id="link-ref",
            ),
            pytest.param(
                b"[text][]",
                b"text",
                b"text",
                id="link-ref, collapsed",
            ),
            pytest.param(
                b"[text]",
                b"text",
                b"text",
                id="link-ref, shortcut",
            ),
            pytest.param(
                b"[][id]",
                b"",
                b"id",
                id="link-ref-with-empty-text",
            ),
            pytest.param(
                b"[text \\]][id]",
                b"text \\]",
                b"id",
                id="link-ref-with-escaped-brackets-in-text",
            ),
        ],
    )
    def test_link_ref(
        self, md: bytes, expected_text: bytes, expected_id: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == expected_text
        assert text(md, link_refs[0].id) == expected_id

    @pytest.mark.parametrize(
        ("md", "expected_text", "expected_id"),
        [
            pytest.param(
                b"[text][id]{}",
                b"text",
                b"id",
                id="link-ref-with-attr",
            ),
            pytest.param(
                b"[text][]{}",
                b"text",
                b"text",
                id="link-ref-with-attr, collapsed",
            ),
            pytest.param(
                b"[text]{}",
                b"text",
                b"text",
                id="link-ref-with-attr, shortcut",
            ),
            pytest.param(
                b"[text][id]{ .class }",
                b"text",
                b"id",
                id="link-ref-with-class-attr",
            ),
            pytest.param(
                b"[text][id]{ #id }",
                b"text",
                b"id",
                id="link-ref-with-id-attr",
            ),
            pytest.param(
                b'[text][id]{ target="_blank" }',
                b"text",
                b"id",
                id="link-ref-with-target-attr",
            ),
            pytest.param(
                b'[text][id]{ .class #id target="_blank" }',
                b"text",
                b"id",
                id="link-ref-with-multiple-attrs",
            ),
            pytest.param(
                b'[text][id] { target="_blank" }',
                b"text",
                b"id",
                id="link-ref-with-spaced-attr, invalid",
            ),
        ],
    )
    def test_link_ref_with_attr_list(
        self, md: bytes, expected_text: bytes, expected_id: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == expected_text
        assert text(md, link_refs[0].id) == expected_id

    def test_link_ref_with_space(self) -> None:
        md = b"[id] (href)"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"id"
        assert text(md, link_refs[0].id) == b"id"

    def test_link_ref_with_footnote(self) -> None:
        md = b"[id][^note]"
        refs = collect(md)
        assert len(refs) == 2

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"id"
        assert text(md, link_refs[0].id) == b"id"

        note_refs = footnote_refs_only(refs)
        assert len(note_refs) == 1
        assert text(md, note_refs[0].id) == b"note"

    def test_link_ref_text_with_newline(self) -> None:
        md = b"[text\nmore text][id]"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"text\nmore text"
        assert text(md, link_refs[0].id) == b"id"

    def test_link_ref_text_brackets_in_id(self) -> None:
        md = b"[text][[id]]"
        refs = collect(md)
        assert len(refs) == 2

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"text"
        assert text(md, link_refs[0].id) == b"text"

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "wikilink"
        assert text(md, links[0].text) == b"id"
        assert text(md, links[0].href) == b"id"

    # --- negative cases ---

    def test_no_link_ref_escaped_brackets(self) -> None:
        md = b"\\[text\\]"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_link_ref_escaped_opening_bracket(self) -> None:
        md = b"\\[text]"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_link_ref_escaped_closing_bracket(self) -> None:
        md = b"[text\\]"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_link_ref_empty_shortcut(self) -> None:
        md = b"[]"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_link_ref_empty_brackets(self) -> None:
        md = b"[][]"
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestLinkDefinitions:
    """Tests for link definitions."""

    @pytest.mark.parametrize(
        ("md", "expected_id", "expected_href"),
        [
            pytest.param(
                b"[id]: href",
                b"id",
                b"href",
                id="link-def",
            ),
            pytest.param(
                b"[multi word id]: href",
                b"multi word id",
                b"href",
                id="link-def, multi-word",
            ),
            pytest.param(
                b'[id]: href "Title"',
                b"id",
                b"href",
                id="link-def-with-title",
            ),
            pytest.param(
                b"[id]: href 'Title'",
                b"id",
                b"href",
                id="link-def-with-title, single quotes",
            ),
            pytest.param(
                b"   [id]: href\n",
                b"id",
                b"href",
                id="link-def-with-indent",
            ),
        ],
    )
    def test_link_def(
        self, md: bytes, expected_id: bytes, expected_href: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_defs = link_defs_only(refs)
        assert len(link_defs) == 1
        assert text(md, link_defs[0].id) == expected_id
        assert text(md, link_defs[0].href) == expected_href

    @pytest.mark.parametrize(
        ("md", "expected_id", "expected_href"),
        [
            pytest.param(
                b"[id]: <href>\nid",
                b"id",
                b"href",
                id="link-def",
            ),
            pytest.param(
                b"[multi word id]: <href>",
                b"multi word id",
                b"href",
                id="link-def, multi-word",
            ),
            pytest.param(
                b'[id]: <href> "Title"',
                b"id",
                b"href",
                id="link-def-with-title",
            ),
            pytest.param(
                b"   [id]: <href>\n",
                b"id",
                b"href",
                id="link-def-with-indent",
            ),
        ],
    )
    def test_link_def_with_angle_brackets(
        self, md: bytes, expected_id: bytes, expected_href: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_defs = link_defs_only(refs)
        assert len(link_defs) == 1
        assert text(md, link_defs[0].id) == expected_id
        assert text(md, link_defs[0].href) == expected_href

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(b'[id]: href\n  "Title"', id="lf"),
            pytest.param(b'[id]: href\r\n  "Title"', id="crlf"),
            pytest.param(b'[id]: href\r  "Title"', id="cr"),
        ],
    )
    def test_link_def_title_on_next_line(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_defs = link_defs_only(refs)
        assert len(link_defs) == 1
        assert text(md, link_defs[0].id) == b"id"
        assert text(md, link_defs[0].href) == b"href"

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"[id]: <href>\r\n[after](href)",
                id="crlf",
            ),
            pytest.param(
                b"[id]: <href>\r[after](href)",
                id="cr",
            ),
        ],
    )
    def test_link_def_angle_brackets_with_link_after(
        self, md: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 2

        link_defs = link_defs_only(refs)
        assert len(link_defs) == 1
        assert text(md, link_defs[0].id) == b"id"
        assert text(md, link_defs[0].href) == b"href"

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"after"

    # --- negative cases ---

    def test_no_link_def_empty_href(self) -> None:
        md = b"[id]:"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1

    def test_no_link_def_indent(self) -> None:
        md = b"    [id]: href"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1

    def test_no_link_def_prefix(self) -> None:
        md = b"text [id]: href"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1


# ---------------------------------------------------------------------------


class TestImages:
    """Tests for images."""

    @pytest.mark.parametrize(
        ("md", "expected_alt", "expected_href"),
        [
            pytest.param(
                b"![alt](image.png)",
                b"alt",
                b"image.png",
                id="image",
            ),
            pytest.param(
                b'![alt](image.png "Title")',
                b"alt",
                b"image.png",
                id="image-with-title",
            ),
            pytest.param(
                b"![](image.png)",
                b"",
                b"image.png",
                id="image-with-empty-alt",
            ),
            pytest.param(
                b"![alt]()",
                b"alt",
                b"",
                id="image-with-empty-href",
            ),
            pytest.param(
                b"![]()",
                b"",
                b"",
                id="image-with-empty-alt-and-href",
            ),
            pytest.param(
                b"![alt [[]]](image.png)",
                b"alt [[]]",
                b"image.png",
                id="image-with-brackets-in-alt",
            ),
            pytest.param(
                b"![alt \\]](image.png)",
                b"alt \\]",
                b"image.png",
                id="image-with-escaped-brackets-in-alt",
            ),
            pytest.param(
                b"![alt]((href))",
                b"alt",
                b"(href)",
                id="image-with-parens-in-href",
            ),
        ],
    )
    def test_image(
        self, md: bytes, expected_alt: bytes, expected_href: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "image"
        assert text(md, links[0].text) == expected_alt
        assert text(md, links[0].href) == expected_href

    @pytest.mark.parametrize(
        ("md", "expected_text", "expected_href"),
        [
            pytest.param(
                b"![alt](image.png){}",
                b"alt",
                b"image.png",
                id="image-with-attr",
            ),
            pytest.param(
                b"![alt](image.png){ .class }",
                b"alt",
                b"image.png",
                id="image-with-class-attr",
            ),
            pytest.param(
                b"![alt](image.png){ #id }",
                b"alt",
                b"image.png",
                id="image-with-id-attr",
            ),
            pytest.param(
                b'![alt](image.png){ loading="lazy" }',
                b"alt",
                b"image.png",
                id="image-with-loading-attr",
            ),
            pytest.param(
                b'![alt](image.png){ .class #id loading="lazy" }',
                b"alt",
                b"image.png",
                id="image-with-multiple-attrs",
            ),
            pytest.param(
                b'![alt](image.png) { loading="lazy" }',
                b"alt",
                b"image.png",
                id="image-with-spaced-attr, invalid",
            ),
        ],
    )
    def test_image_with_attr_list(
        self, md: bytes, expected_text: bytes, expected_href: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "image"
        assert text(md, links[0].text) == expected_text
        assert text(md, links[0].href) == expected_href

    def test_image_with_angle_brackets(self) -> None:
        md = b"![alt](<image.png>)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "image"
        assert text(md, links[0].text) == b"alt"
        assert text(md, links[0].href) == b"image.png"

    def test_image_ref_with_space(self) -> None:
        md = b"![id] (href)"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "image"
        assert text(md, link_refs[0].text) == b"id"
        assert text(md, link_refs[0].id) == b"id"

    def test_image_text_with_newline(self) -> None:
        md = b"![alt\nmore text](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "image"
        assert text(md, links[0].text) == b"alt\nmore text"
        assert text(md, links[0].href) == b"href"

    # --- negative cases ---

    def test_no_image_escaped_bang(self) -> None:
        md = b"\\![alt](image.png)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == b"alt"
        assert text(md, links[0].href) == b"image.png"

    def test_no_image_spaced_bang(self) -> None:
        md = b"! [alt](image.png)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == b"alt"
        assert text(md, links[0].href) == b"image.png"

    def test_no_image_only_bang(self) -> None:
        md = b"!image.png"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_image_escaped_brackets(self) -> None:
        md = b"!\\[alt\\](image.png)"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_image_escaped_opening_bracket(self) -> None:
        md = b"!\\[alt](image.png)"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_image_escaped_closing_bracket(self) -> None:
        md = b"![alt\\](image.png)"
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestImageReferences:
    """Tests for image references."""

    @pytest.mark.parametrize(
        ("md", "expected_alt", "expected_id"),
        [
            pytest.param(
                b"![alt][id]",
                b"alt",
                b"id",
                id="image-ref",
            ),
            pytest.param(
                b"![alt][]",
                b"alt",
                b"alt",
                id="image-ref, collapsed",
            ),
            pytest.param(
                b"![id]",
                b"id",
                b"id",
                id="image-ref, shortcut",
            ),
            pytest.param(
                b"![][id]",
                b"",
                b"id",
                id="image-ref-with-empty-alt",
            ),
            pytest.param(
                b"![][]",
                b"",
                b"",
                id="image-ref-with-empty-alt-and-id",
            ),
            pytest.param(
                b"![alt [[]]][id]",
                b"alt [[]]",
                b"id",
                id="image-ref-with-brackets-in-text",
            ),
            pytest.param(
                b"![alt \\]][id]",
                b"alt \\]",
                b"id",
                id="image-ref-with-escaped-brackets-in-text",
            ),
        ],
    )
    def test_image_ref(
        self, md: bytes, expected_alt: bytes, expected_id: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "image"
        assert text(md, link_refs[0].text) == expected_alt
        assert text(md, link_refs[0].id) == expected_id

    @pytest.mark.parametrize(
        ("md", "expected_text", "expected_id"),
        [
            pytest.param(
                b"![alt][image-id]{}",
                b"alt",
                b"image-id",
                id="image-ref-with-attr",
            ),
            pytest.param(
                b"![alt][]{}",
                b"alt",
                b"alt",
                id="image-ref-with-attr, collapsed",
            ),
            pytest.param(
                b"![alt]{}",
                b"alt",
                b"alt",
                id="image-ref-with-attr, shortcut",
            ),
            pytest.param(
                b"![alt][id]{ .class }",
                b"alt",
                b"id",
                id="image-ref-with-class-attr",
            ),
            pytest.param(
                b"![alt][id]{ #id }",
                b"alt",
                b"id",
                id="image-ref-with-id-attr",
            ),
            pytest.param(
                b'![alt][id]{ loading="lazy" }',
                b"alt",
                b"id",
                id="image-ref-with-loading-attr",
            ),
            pytest.param(
                b'![alt][id]{ .class #id loading="lazy" }',
                b"alt",
                b"id",
                id="image-ref-with-multiple-attrs",
            ),
            pytest.param(
                b'![alt][id] { loading="lazy" }',
                b"alt",
                b"id",
                id="image-ref-with-spaced-attr, invalid",
            ),
        ],
    )
    def test_image_ref_with_attr_list(
        self, md: bytes, expected_text: bytes, expected_id: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "image"
        assert text(md, link_refs[0].text) == expected_text
        assert text(md, link_refs[0].id) == expected_id

    def test_image_ref_text_with_newline(self) -> None:
        md = b"![alt\nmore text][id]"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "image"
        assert text(md, link_refs[0].text) == b"alt\nmore text"
        assert text(md, link_refs[0].id) == b"id"

    # --- negative cases ---

    def test_no_image_ref_escaped_bang(self) -> None:
        md = b"\\![alt][id]"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"alt"
        assert text(md, link_refs[0].id) == b"id"

    def test_no_image_ref_escaped_brackets(self) -> None:
        md = b"!\\[text\\]"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_image_ref_escaped_opening_bracket(self) -> None:
        md = b"!\\[text]"
        refs = collect(md)
        assert len(refs) == 0

    def test_no_image_ref_escaped_closing_bracket(self) -> None:
        md = b"![text\\]"
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestInnerLinks:
    """Tests for links in links and images."""

    def test_link_ref_in_link(self) -> None:
        md = b"[text[more]](href)"
        refs = collect(md)
        assert len(refs) == 2

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == b"text[more]"
        assert text(md, links[0].href) == b"href"

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"more"
        assert text(md, link_refs[0].id) == b"more"

    def test_link_ref_in_link_ref(self) -> None:
        md = b"[text[more]][id]"
        refs = collect(md)
        assert len(refs) == 2

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 2
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"text[more]"
        assert text(md, link_refs[0].id) == b"id"
        assert link_refs[1].kind == "link"
        assert text(md, link_refs[1].text) == b"more"
        assert text(md, link_refs[1].id) == b"more"

    def test_image_in_link(self) -> None:
        md = b"[![alt](image.png)](href)"
        refs = collect(md)
        assert len(refs) == 2

        links = links_only(refs)
        assert len(links) == 2
        assert links[0].kind == "link"
        assert text(md, links[0].href) == b"href"
        assert links[1].kind == "image"
        assert text(md, links[1].text) == b"alt"
        assert text(md, links[1].href) == b"image.png"

    def test_image_ref_in_link(self) -> None:
        md = b"[![alt][image-id]](href)"
        refs = collect(md)
        assert len(refs) == 2

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].href) == b"href"

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "image"
        assert text(md, link_refs[0].text) == b"alt"
        assert text(md, link_refs[0].id) == b"image-id"

    def test_image_in_link_ref(self) -> None:
        md = b"[![alt](image.png)][id]"
        refs = collect(md)
        assert len(refs) == 2

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "image"
        assert text(md, links[0].text) == b"alt"
        assert text(md, links[0].href) == b"image.png"

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].id) == b"id"

    def test_image_ref_in_link_ref(self) -> None:
        md = b"[![alt][image-id]][id]"
        refs = collect(md)
        assert len(refs) == 2

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 2
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].id) == b"id"
        assert link_refs[1].kind == "image"
        assert text(md, link_refs[1].text) == b"alt"
        assert text(md, link_refs[1].id) == b"image-id"

    def test_no_link_ref_in_link(self) -> None:
        md = b"[text\\[more\\]](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == b"text\\[more\\]"
        assert text(md, links[0].href) == b"href"

    def test_no_link_ref_in_link_ref(self) -> None:
        md = b"[text\\[more\\]][id]"
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"text\\[more\\]"
        assert text(md, link_refs[0].id) == b"id"


# ---------------------------------------------------------------------------


class TestAutolinks:
    """Tests for autolinks."""

    @pytest.mark.parametrize(
        ("md", "expected_href"),
        [
            pytest.param(
                b"<https://example.com>",
                b"https://example.com",
                id="autolink-https",
            ),
            pytest.param(
                b"<http://example.com>",
                b"http://example.com",
                id="autolink-http",
            ),
        ],
    )
    def test_autolink(self, md: bytes, expected_href: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "autolink"
        assert text(md, links[0].text) == expected_href
        assert text(md, links[0].href) == expected_href

    # --- negative cases ---

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"<not-a-url>",
                id="autolink-scheme, missing",
            ),
            pytest.param(
                b"<ftp://example.com>",
                id="autolink-scheme, unsupported",
            ),
            pytest.param(
                b"<>",
                id="autolink-empty",
            ),
        ],
    )
    def test_no_autolink(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestWikilinks:
    """Tests for wikilinks."""

    @pytest.mark.parametrize(
        ("md", "expected_text"),
        [
            pytest.param(
                b"[[Page]]",
                b"Page",
                id="wikilink",
            ),
            pytest.param(
                b"[[Page|display]]",
                b"Page|display",
                id="wikilink-with-text",
            ),
        ],
    )
    def test_wikilink(self, md: bytes, expected_text: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "wikilink"
        assert text(md, links[0].text) == expected_text
        assert text(md, links[0].href) == expected_text

    # --- negative cases ---

    @pytest.mark.parametrize(
        ("md", "expected_text", "expected_id"),
        [
            pytest.param(
                b"[[Page]][id]",
                b"[Page]",
                b"id",
                id="link-ref",
            ),
            pytest.param(
                b"[[Page]][]",
                b"[Page]",
                b"[Page]",
                id="link-ref, collapsed",
            ),
            pytest.param(
                b"[[Page]]\n[id]",
                b"[Page]",
                b"id",
                id="link-ref-with-newline",
            ),
            pytest.param(
                b"[[Page]]\r\n[id]",
                b"[Page]",
                b"id",
                id="link-ref-with-crlf",
            ),
            pytest.param(
                b"[[Page]]\r[id]",
                b"[Page]",
                b"id",
                id="link-ref-with-cr",
            ),
        ],
    )
    def test_no_wikilink(
        self, md: bytes, expected_text: bytes, expected_id: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 2

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 2
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == expected_text
        assert text(md, link_refs[0].id) == expected_id
        assert link_refs[1].kind == "link"
        assert text(md, link_refs[1].text) == b"Page"
        assert text(md, link_refs[1].id) == b"Page"


# ---------------------------------------------------------------------------


class TestFootnoteReferences:
    """Tests for footnote references."""

    @pytest.mark.parametrize(
        ("md", "expected_id"),
        [
            pytest.param(
                b"[^1]",
                b"1",
                id="footnote-ref",
            ),
            pytest.param(
                b"[^note]",
                b"note",
                id="footnote-ref-with-name",
            ),
            pytest.param(
                b"[^@#$%]",
                b"@#$%",
                id="footnote-ref-with-special-chars",
            ),
        ],
    )
    def test_footnote_ref(self, md: bytes, expected_id: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        note_refs = footnote_refs_only(refs)
        assert len(note_refs) == 1
        assert text(md, note_refs[0].id) == expected_id

    # --- negative cases ---

    @pytest.mark.parametrize(
        ("md", "expected_id"),
        [
            pytest.param(
                b"[^]",
                b"^",
                id="link-ref",
            ),
            pytest.param(
                b"[^ ]",
                b"^ ",
                id="link-ref, space",
            ),
            pytest.param(
                b"[^multi word note]",
                b"^multi word note",
                id="link-ref, separated",
            ),
        ],
    )
    def test_no_footnote_ref(self, md: bytes, expected_id: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == expected_id
        assert text(md, link_refs[0].id) == expected_id


# ---------------------------------------------------------------------------


class TestFootnoteDefinitions:
    """Tests for footnote definitions."""

    @pytest.mark.parametrize(
        ("md", "expected_id", "expected_body"),
        [
            pytest.param(
                b"[^1]: body",
                b"1",
                b"body",
                id="footnote-def",
            ),
            pytest.param(
                b"[^note]: body",
                b"note",
                b"body",
                id="footnote-def-with-name",
            ),
            pytest.param(
                b"[^@#$%]: body",
                b"@#$%",
                b"body",
                id="footnote-def-with-special-chars",
            ),
            pytest.param(
                b"[^1]: body\n    continuation\n    and so on.",
                b"1",
                b"body\n    continuation\n    and so on.",
                id="footnote-def, multiple lines",
            ),
        ],
    )
    def test_footnote_def(
        self, md: bytes, expected_id: bytes, expected_body: bytes
    ) -> None:
        refs = collect(md)
        assert len(refs) == 1

        note_defs = footnote_defs_only(refs)
        assert len(note_defs) == 1
        assert text(md, note_defs[0].id) == expected_id
        assert text(md, note_defs[0].body) == expected_body

    def test_footnote_def_with_link(self) -> None:
        md = b"[^1]: [text](href)"
        refs = collect(md)
        assert len(refs) == 2

        note_defs = footnote_defs_only(refs)
        assert len(note_defs) == 1
        assert text(md, note_defs[0].id) == b"1"

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "link"
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"href"

    def test_footnote_def_with_link_ref(self) -> None:
        md = b"[^1]: [id]"
        refs = collect(md)
        assert len(refs) == 2

        note_defs = footnote_defs_only(refs)
        assert len(note_defs) == 1
        assert text(md, note_defs[0].id) == b"1"

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "link"
        assert text(md, link_refs[0].text) == b"id"
        assert text(md, link_refs[0].id) == b"id"

    def test_footnote_def_with_link_def(self) -> None:
        md = b"[^1]: body\n[id]: href"
        refs = collect(md)
        assert len(refs) == 2

        note_defs = footnote_defs_only(refs)
        assert len(note_defs) == 1
        assert text(md, note_defs[0].id) == b"1"

        link_defs = link_defs_only(refs)
        assert len(link_defs) == 1
        assert text(md, link_defs[0].id) == b"id"
        assert text(md, link_defs[0].href) == b"href"

    def test_footnote_def_with_image(self) -> None:
        md = b"[^1]: ![alt](href)"
        refs = collect(md)
        assert len(refs) == 2

        note_defs = footnote_defs_only(refs)
        assert len(note_defs) == 1
        assert text(md, note_defs[0].id) == b"1"

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "image"
        assert text(md, links[0].text) == b"alt"
        assert text(md, links[0].href) == b"href"

    def test_footnote_def_with_image_ref(self) -> None:
        md = b"[^1]: ![id]"
        refs = collect(md)
        assert len(refs) == 2

        note_defs = footnote_defs_only(refs)
        assert len(note_defs) == 1
        assert text(md, note_defs[0].id) == b"1"

        link_refs = link_refs_only(refs)
        assert len(link_refs) == 1
        assert link_refs[0].kind == "image"
        assert text(md, link_refs[0].text) == b"id"
        assert text(md, link_refs[0].id) == b"id"

    def test_no_footnote_def_with_inline_code(self) -> None:
        md = b"[^1]: `[code]`"
        refs = collect(md)
        assert len(refs) == 1

        note_defs = footnote_defs_only(refs)
        assert len(note_defs) == 1

    # --- negative cases ---

    def test_no_footnote_def_prefix(self) -> None:
        md = b"text [^1]: body\n"
        refs = collect(md)
        assert len(refs) == 1

        note_refs = footnote_refs_only(refs)
        assert len(note_refs) == 1


# ---------------------------------------------------------------------------


class TestFencedCodeBlocks:
    """Tests for fenced code blocks."""

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"```\n[text](href)\n```",
                id="fenced-code-3-backticks",
            ),
            pytest.param(
                b"``` python\n[text](href)\n```",
                id="fenced-code-3-backticks-lang",
            ),
            pytest.param(
                b"````\n[text](href)\n````",
                id="fenced-code-4-backticks",
            ),
            pytest.param(
                b"```` py\n[text](href)\n````",
                id="fenced-code-4-backticks-lang",
            ),
            pytest.param(
                b"~~~\n[text](href)\n~~~",
                id="fenced-code-3-tildes",
            ),
            pytest.param(
                b"~~~ python\n[text](href)\n~~~",
                id="fenced-code-3-tildes-lang",
            ),
            pytest.param(
                b"~~~~\n[text](href)\n~~~~",
                id="fenced-code-4-tildes",
            ),
            pytest.param(
                b"~~~~ py\n[text](href)\n~~~~",
                id="fenced-code-4-tildes-lang",
            ),
            pytest.param(
                b"\n```\n[Start][]\n```\n",
                id="fenced-code-with-info",
            ),
            pytest.param(
                b"```\r\n[Start]\r\n```\r\n",
                id="fenced-code-crlf-with-shortcut-link-ref",
            ),
            pytest.param(
                b"```\r[Start]\r```\r",
                id="fenced-code-cr-with-shortcut-link-ref",
            ),
        ],
    )
    def test_fenced_code(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0

    def test_fenced_code_with_link_after(self) -> None:
        md = b"```\n[text](href)\n```\n[after](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"after"
        assert text(md, links[0].href) == b"href"

    def test_fenced_code_with_link_before(self) -> None:
        md = b"[before](href)\n```\n[text](href)\n```"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"before"
        assert text(md, links[0].href) == b"href"

    # --- negative cases ---

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"```\n[text](href)\n````",
                id="fenced-code-unbalanced-backticks",
            ),
            pytest.param(
                b"```\n[text](href)",
                id="fenced-code-missing-backticks",
            ),
            pytest.param(
                b"```\n[text](href)\n~~~",
                id="fenced-code-backticks-tildes",
            ),
            pytest.param(
                b"~~~\n[text](href)\n```",
                id="fenced-code-tildes-backticks",
            ),
        ],
    )
    def test_no_fenced_code(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"href"


# ---------------------------------------------------------------------------


class TestInlineCode:
    """Tests for inline code."""

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"`[text](href)`",
                id="inline-code",
            ),
            pytest.param(
                b"``[text](href)``",
                id="inline-code-2-backticks",
            ),
            pytest.param(
                b"`` [text](href) ``",
                id="inline-code-2-backticks, spaces",
            ),
            pytest.param(
                b"```[text](href)```",
                id="inline-code-3-backticks",
            ),
            pytest.param(
                b"``text`[text](href)``",
                id="inline-code, inner backticks",
            ),
            pytest.param(
                b"`` `[text](href)`",
                id="inline-code, empty code span",
            ),
        ],
    )
    def test_inline_code(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0

    def test_inline_code_with_link_after(self) -> None:
        md = b"`code` [text](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"href"

    def test_inline_code_with_link_before(self) -> None:
        md = b"[text](href) `code`"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"href"

    # --- negative cases ---

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"`[text](href)\n``",
                id="inline-code-unbalanced-backticks",
            ),
            pytest.param(
                b"`\n[text](href)",
                id="inline-code-missing-backticks",
            ),
        ],
    )
    def test_no_inline_code(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"href"


# ---------------------------------------------------------------------------


class TestMath:
    """Tests for math blocks and inline math."""

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"$$\n[text](href)\n$$",
                id="math-block",
            ),
            pytest.param(
                b"$$ [text](href) $$",
                id="math-block, single line",
            ),
            pytest.param(
                b"$[text](href)$",
                id="math-inline",
            ),
            pytest.param(
                b"\\[\n[text](href)\n\\]",
                id="match-block-brackets",
            ),
            pytest.param(
                b"\\[\r\n[text](href)\r\n\\]",
                id="match-block-brackets-crlf",
            ),
            pytest.param(
                b"\\[\r[text](href)\r\\]",
                id="match-block-brackets-cr",
            ),
            pytest.param(
                b"\\[ [text](href) \\]",
                id="match-block-brackets, single line",
            ),
            pytest.param(
                b"\\([text](href)\\)",
                id="math-inline-parens",
            ),
        ],
    )
    def test_math(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0

    def test_math_with_link_after(self) -> None:
        md = b"$$\n[text](href)\n$$\n[after](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"after"
        assert text(md, links[0].href) == b"href"

    def test_math_with_link_before(self) -> None:
        md = b"[before](href)\n$$\n[text](href)\n$$"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"before"
        assert text(md, links[0].href) == b"href"

    def test_no_math(self) -> None:
        md = b"1.000$"
        refs = collect(md)
        assert len(refs) == 0

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(b"$[text](href)\n$", id="lf"),
            pytest.param(b"$[text](href)\r\n$", id="crlf"),
            pytest.param(b"$[text](href)\r$", id="cr"),
        ],
    )
    def test_no_math_inline_across_line_ending(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"


# ---------------------------------------------------------------------------


class TestHtmlLinks:
    """Tests for HTML elements with links."""

    @pytest.mark.parametrize(
        ("md", "expected_href"),
        [
            pytest.param(
                b'<a href="href">text</a>',
                b"href",
                id="html-a-href",
            ),
            pytest.param(
                b'<div>\n<a href="href">text</a>\n</div>',
                b"href",
                id="html-a-href, in block",
            ),
            pytest.param(
                b'<div>\r\n<a href="href">text</a>\r\n</div>',
                b"href",
                id="html-a-href, in block, crlf",
            ),
            pytest.param(
                b'<div>\r<a href="href">text</a>\r</div>',
                b"href",
                id="html-a-href, in block, cr",
            ),
            pytest.param(
                b'<img src="image.png">',
                b"image.png",
                id="html-img-src",
            ),
            pytest.param(
                b'<img src="image.png" />',
                b"image.png",
                id="html-img-src, self-closing",
            ),
            pytest.param(
                b'<div>\n<img src="image.png">\n</div>',
                b"image.png",
                id="html-img-src, in block",
            ),
            pytest.param(
                b'<p>\n<img\nsrc="image.png"\n>\n</p>',
                b"image.png",
                id="html-p-img-src, in block, multiple lines",
            ),
            pytest.param(
                b'<link href="style.css">',
                b"style.css",
                id="html-link-href",
            ),
            pytest.param(
                b'<script src="script.js"></script>',
                b"script.js",
                id="html-script-src",
            ),
            pytest.param(
                b'<audio src="audio.mp3">',
                b"audio.mp3",
                id="html-audio-src",
            ),
            pytest.param(
                b'<video src="video.mp4">',
                b"video.mp4",
                id="html-video-src",
            ),
            pytest.param(
                b'<p>\n<img src="image.png">\n</p>',
                b"image.png",
                id="html-p-img-src, in block",
            ),
        ],
    )
    def test_html_link(self, md: bytes, expected_href: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].kind == "html"
        assert text(md, links[0].text) == expected_href
        assert text(md, links[0].href) == expected_href

    # --- negative cases ---

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b'<!-- <img src="image.png"> -->',
                id="html-link, in comment",
            ),
            pytest.param(
                b'<div>\n<!-- <img src="image.png"> -->\n</div>',
                id="html-link, in comment in block",
            ),
        ],
    )
    def test_no_html_link(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestHtmlComments:
    """Tests for HTML comments."""

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"<!-- [text](href) -->",
                id="html-comment",
            ),
            pytest.param(
                b"<!--\n[text](href)\n-->",
                id="html-comment-multi-line",
            ),
        ],
    )
    def test_html_comment(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestHtmlBlocks:
    """Tests for HTML blocks."""

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"<div>\n[text](href)\n</div>",
                id="html-block, div",
            ),
            pytest.param(
                b"<p>\n[text](href)\n</p>",
                id="html-block, p",
            ),
            pytest.param(
                b"<blockquote>\n[text](href)\n</blockquote>",
                id="html-block, blockquote",
            ),
            pytest.param(
                b"<ul>\n[text](href)\n</ul>",
                id="html-block, ul",
            ),
        ],
    )
    def test_html_block(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"<div markdown>\n[text](href)\n</div>",
                id="html-block, div",
            ),
            pytest.param(
                b"<p markdown>\n[text](href)\n</p>",
                id="html-block, p",
            ),
            pytest.param(
                b"<blockquote markdown>\n[text](href)\n</blockquote>",
                id="html-block, blockquote",
            ),
            pytest.param(
                b"<ul markdown>\n[text](href)\n</ul>",
                id="html-block, ul",
            ),
        ],
    )
    def test_html_block_with_markdown_attr(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"href"

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"<a>\n[text](href)\n</a>",
                id="html-inline, a",
            ),
            pytest.param(
                b"<small>[text](href)</small>",
                id="html-inline, small",
            ),
            pytest.param(
                b"<span>[text](href)</span>",
                id="html-inline, span",
            ),
            pytest.param(
                b"<em>[text](href)</em>",
                id="html-inline, em",
            ),
        ],
    )
    def test_html_inline(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"
        assert text(md, links[0].href) == b"href"


# ---------------------------------------------------------------------------


class TestJinja:
    """Tests for Jinja syntax."""

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"{% if [text](href) %}...{% endif %}",
                id="jinja-block",
            ),
            pytest.param(
                b"{% if\r\n[text](href)\r\n%}",
                id="jinja-block-crlf",
            ),
            pytest.param(
                b"{% if\r[text](href)\r%}",
                id="jinja-block-cr",
            ),
            pytest.param(
                b"{{ [text](href) }}",
                id="jinja-expr",
            ),
            pytest.param(
                b"{# [text](href) #}",
                id="jinja-comment",
            ),
        ],
    )
    def test_no_refs_inside_jinja(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(b"{% if\n\n[text](href) %}", id="lf"),
            pytest.param(b"{% if\r\n\r\n[text](href) %}", id="crlf"),
            pytest.param(b"{% if\r\r[text](href) %}", id="cr"),
        ],
    )
    def test_refs_after_jinja_blank_line(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"text"


# ---------------------------------------------------------------------------


class TestExclusions:
    """Tests for further exclusions."""

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(b"*[abbr]: text\n", id="lf"),
            pytest.param(b"*[abbr]: text\r\n", id="crlf"),
            pytest.param(b"*[abbr]: text\r", id="cr"),
        ],
    )
    def test_abbreviations(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"*[abbr]: text\r\n[after](href)",
                id="crlf",
            ),
            pytest.param(
                b"*[abbr]: text\r[after](href)",
                id="cr",
            ),
        ],
    )
    def test_abbreviation_with_link_after(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert text(md, links[0].text) == b"after"

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                b"- [ ] task item\n",
                id="checkbox",
            ),
            pytest.param(
                b"- [x] done item\n",
                id="checkbox, checked",
            ),
            pytest.param(
                b"- [X] done item\n",
                id="checkbox, checked, upper",
            ),
        ],
    )
    def test_tasklists(self, md: bytes) -> None:
        refs = collect(md)
        assert len(refs) == 0


# ---------------------------------------------------------------------------


class TestSpanOffsets:
    """Tests for span offsets in links and references."""

    def test_span(self) -> None:
        md = b"[text](href)"
        refs = collect(md)
        assert len(refs) == 1

        assert refs[0].start == 0
        assert refs[0].end == 12

    def test_span_after(self) -> None:
        md = b"content[text](href)"
        refs = collect(md)
        assert len(refs) == 1

        assert refs[0].start == 7
        assert refs[0].end == 19

    def test_span_after_lf(self) -> None:
        md = b"content\n[text](href)"
        refs = collect(md)
        assert len(refs) == 1

        assert refs[0].start == 8
        assert refs[0].end == 20

    def test_span_after_crlf(self) -> None:
        md = b"content\r\n[text](href)"
        refs = collect(md)
        assert len(refs) == 1

        assert refs[0].start == 9
        assert refs[0].end == 21

    def test_span_after_cr(self) -> None:
        md = b"content\r[text](href)"
        refs = collect(md)
        assert len(refs) == 1

        assert refs[0].start == 8
        assert refs[0].end == 20

    def test_text_span(self) -> None:
        md = b"[text](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].text.start == 1
        assert links[0].text.end == 5

    def test_text_span_multibyte(self) -> None:
        md = "[日本語](href)".encode()
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].text.start == 1
        assert links[0].text.end == 10

    def test_href_span(self) -> None:
        md = b"[text](href)"
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].href.start == 7
        assert links[0].href.end == 11

    def test_href_span_multibyte(self) -> None:
        md = "[text](例え.jp)".encode()
        refs = collect(md)
        assert len(refs) == 1

        links = links_only(refs)
        assert len(links) == 1
        assert links[0].href.start == 7
        assert links[0].href.end == 16


# ---------------------------------------------------------------------------
# Helper functions
# ---------------------------------------------------------------------------


def collect(markdown: bytes, *, shift: int = 0) -> list[Reference]:
    """Return all references as a plain list."""
    return list(references(markdown, shift=shift))


def text(markdown: bytes, span: Span) -> bytes:
    """Return the slice that the given span covers."""
    return markdown[span.start : span.end]


# ---------------------------------------------------------------------------


def links_only(refs: list) -> list[Link]:
    """Filter to links or images only."""
    return [ref for ref in refs if isinstance(ref, Link)]


def link_refs_only(refs: list) -> list[LinkReference]:
    """Filter to link or image references only."""
    return [ref for ref in refs if isinstance(ref, LinkReference)]


def link_defs_only(refs: list) -> list[LinkDefinition]:
    """Filter to link or image definitions instances only."""
    return [ref for ref in refs if isinstance(ref, LinkDefinition)]


def footnote_refs_only(refs: list) -> list[FootnoteReference]:
    """Filter to footnote references only."""
    return [ref for ref in refs if isinstance(ref, FootnoteReference)]


def footnote_defs_only(refs: list) -> list[FootnoteDefinition]:
    """Filter to footnote definitions only."""
    return [ref for ref in refs if isinstance(ref, FootnoteDefinition)]
