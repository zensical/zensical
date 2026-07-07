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

import re
from abc import ABC, abstractmethod
from dataclasses import dataclass
from html import escape
from typing import TYPE_CHECKING, Any
from xml.etree.ElementTree import Element

from markdown.core import Markdown
from markdown.extensions import Extension
from markdown.extensions.toc import slugify
from markdown.inlinepatterns import (
    REFERENCE_RE,
    ReferenceInlineProcessor,
)
from markdown.treeprocessors import Treeprocessor
from markdown.util import HTML_PLACEHOLDER_RE, INLINE_PLACEHOLDER_RE
from markupsafe import Markup

from zensical.extensions.context import ContextPreprocessor

if TYPE_CHECKING:
    from pathlib import Path
    from re import Match

    from markdown import Markdown

    from zensical.extensions.context import Page


# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------


HTAGS = {"h1", "h2", "h3", "h4", "h5", "h6"}
AUTOREF_RE = re.compile(
    r"<autoref (?P<attrs>.*?)>(?P<title>.*?)</autoref>", flags=re.DOTALL
)


# ----------------------------------------------------------------------------
# Globals
# ----------------------------------------------------------------------------


AUTOREFS: AutorefsStore | None = None


# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class AutorefsStore:
    """Mock the autorefs plugin (data store)."""

    def __init__(self) -> None:
        self.current_page: Page | None = None
        self.scan_toc: bool = True
        self.record_backlinks: bool = False

        self._primary_url_map: dict[str, list[str]] = {}
        self._secondary_url_map: dict[str, list[str]] = {}
        self._abs_url_map: dict[str, str] = {}
        self._title_map: dict[str, str] = {}

    def register_anchor(
        self,
        page: Page,
        identifier: str,
        anchor: str | None = None,
        *,
        title: str | None = None,
        primary: bool = True,
    ) -> None:
        url = f"{page.url}#{anchor or identifier}"
        url_map = self._primary_url_map if primary else self._secondary_url_map
        if identifier in url_map:
            if url not in url_map[identifier]:
                url_map[identifier].append(url)
        else:
            url_map[identifier] = [url]
        if title and url not in self._title_map:
            self._title_map[url] = title

    def register_url(self, identifier: str, url: str) -> None:
        self._abs_url_map[identifier] = url


# Unusued yet, only when/if we vendor mkdocstrings and handlers
class AutorefsHookInterface(ABC):
    """An interface for hooking into how AutoRef handles inline references."""

    @dataclass
    class Context:
        """The context around an auto-reference."""

        domain: str
        role: str
        origin: str
        filepath: str | Path
        lineno: int

        def as_dict(self) -> dict[str, str]:
            """Convert the context to a dictionary of HTML attributes."""
            return {
                "domain": self.domain,
                "role": self.role,
                "origin": self.origin,
                "filepath": str(self.filepath),
                "lineno": str(self.lineno),
            }

    @abstractmethod
    def expand_identifier(self, identifier: str) -> str:
        """Expand an identifier in a given context."""
        raise NotImplementedError

    @abstractmethod
    def get_context(self) -> AutorefsHookInterface.Context:
        """Get the current context."""
        raise NotImplementedError


class AutorefsInlineProcessor(ReferenceInlineProcessor):
    """A Markdown extension to handle inline references."""

    # We have to keep the legacy name `mkdocs-autorefs` because mkdocstrings
    # checks that the name of the original inline processor is a member of
    # `self.md.inlinepatterns` to know whether it must assign the hook.
    # This can be changed to something else if we update mkdocstrings.
    name = "mkdocs-autorefs"
    hook: AutorefsHookInterface | None = None

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(REFERENCE_RE, *args, **kwargs)

    @property
    def stashed_nodes(self) -> dict[str, Element | str]:
        return self.md.treeprocessors["inline"].stashed_nodes

    def handleMatch(
        self, m: Match[str], data: str
    ) -> tuple[Element | None, int | None, int | None]:
        """Handle an element that matched."""
        text, index, handled = self.getText(data, m.end(0))
        if not handled:
            return None, None, None

        identifier, slug, end, handled = self._eval_id(data, index, text)
        if not handled or identifier is None:
            return None, None, None

        if slug is None and re.search(r"[\x00-\x1f]", identifier):
            # Do nothing if the matched reference still contains control
            # characters (from 0 to 31 included) that weren't unstashed when
            # trying to compute a slug of the title.
            return None, m.start(0), end

        return self._make_tag(identifier, text, slug=slug), m.start(0), end

    def _unstash(self, identifier: str) -> str:
        stashed_nodes = self.stashed_nodes

        def _repl(match: Match) -> str:
            el = stashed_nodes.get(match[1])
            if isinstance(el, Element):
                return f"`{''.join(el.itertext())}`"
            if el == "\x0296\x03":
                return "`"
            return str(el)

        return INLINE_PLACEHOLDER_RE.sub(_repl, identifier)

    def _eval_id(
        self, data: str, index: int, text: str
    ) -> tuple[str | None, str | None, int, bool]:
        """Evaluate the id portion of `[ref][id]`.

        If `[ref][]` use `[ref]`.
        """
        m = self.RE_LINK.match(data, pos=index)
        if not m:
            return None, None, index, False

        # Default; an identifier was provided, match it exactly (later).
        slug = None

        if not (identifier := m.group(1)):
            # Only a title was provided, use it as identifier.
            identifier = text

            # Catch single stash entries, like the result of [`Foo`][].
            if match := INLINE_PLACEHOLDER_RE.fullmatch(identifier):
                stashed_nodes = self.stashed_nodes
                el = stashed_nodes.get(match[1])
                if isinstance(el, Element) and el.tag == "code":
                    # The title was wrapped in backticks, we only keep the
                    # content and tell autorefs to match the identifier exactly.
                    identifier = "".join(el.itertext())
                    # Special case: allow pymdownx.inlinehilite raw <code>
                    # snippets but strip them back to unhighlighted.
                    if match := HTML_PLACEHOLDER_RE.fullmatch(identifier):
                        stash_index = int(match.group(1))
                        html = self.md.htmlStash.rawHtmlBlocks[stash_index]
                        identifier = Markup(html).striptags()  # noqa: S704
                        self.md.htmlStash.rawHtmlBlocks[stash_index] = escape(
                            identifier
                        )

            # In any other case, unstash the title and slugify it.
            # Examples: ``[`Foo` and `Bar`]``, `[The *Foo*][]`.
            else:
                identifier = self._unstash(identifier)
                slug = slugify(identifier, separator="-")

        end = m.end(0)
        return identifier, slug, end, True

    def _make_tag(
        self, identifier: str, text: str, *, slug: str | None = None
    ) -> Element:
        """Create a tag that can be matched by `AUTO_REF_RE`."""
        el = Element("autoref")
        if self.hook:
            identifier = self.hook.expand_identifier(identifier)
            el.attrib.update(self.hook.get_context().as_dict())
        el.set("identifier", identifier)
        el.text = text
        if slug:
            el.attrib["slug"] = slug
        return el


class AutorefsAnchorsTreeprocessor(Treeprocessor):
    """Tree processor to scan and register HTML anchors."""

    name = "autorefs-anchors"

    class PendingAnchors:
        """An accumulating collection of HTML anchors."""

        def __init__(self, store: AutorefsStore, page: Page):
            self.store = store
            self.page = page
            self.anchors: list[str] = []

        def append(self, anchor: str) -> None:
            self.anchors.append(anchor)

        def flush(
            self, alias_to: str | None = None, title: str | None = None
        ) -> None:
            for anchor in self.anchors:
                self.store.register_anchor(
                    self.page, anchor, alias_to, title=title, primary=True
                )
            self.anchors.clear()

    def __init__(self, md: Markdown) -> None:
        super().__init__(md)

    def run(self, root: Element) -> None:
        """Run the tree processor."""
        if context := ContextPreprocessor.from_markdown(self.md):
            store = get_autorefs_store()
            pending_anchors = self.PendingAnchors(store, context.page)
            self._scan_anchors(root, pending_anchors)
            pending_anchors.flush()

    def _scan_anchors(
        self,
        parent: Element,
        pending_anchors: PendingAnchors,
        last_heading: str | None = None,
    ) -> None:
        for el in parent:
            if el.tag == "a":
                # We found an anchor. Record its id if it has one.
                if anchor_id := el.get("id"):
                    pending_anchors.append(anchor_id)
                # If the element has text or a link, it's not an alias.
                # Non-whitespace text after the element interrupts the chain,
                # aliases can't apply.
                if el.text or el.get("href") or (el.tail and el.tail.strip()):
                    pending_anchors.flush(title=last_heading)

            elif el.tag == "p":
                # A `p` tag is a no-op for our purposes, just recurse into it
                # in the context of the current collection of anchors.
                self._scan_anchors(el, pending_anchors, last_heading)
                # Non-whitespace text after the element interrupts the chain,
                # aliases can't apply.
                if el.tail and el.tail.strip():
                    pending_anchors.flush()

            elif el.tag in HTAGS:
                # If the element is a heading, that turns the pending anchors
                # into aliases.
                last_heading = el.text
                pending_anchors.flush(el.get("id"), title=last_heading)

            else:
                # But if it's some other interruption, flush anchors anyway
                # as non-aliases.
                pending_anchors.flush(title=last_heading)
                # Recurse into sub-elements, in a *separate* context.
                self.run(el)


class AutorefsHeadingsTreeprocessor(Treeprocessor):
    """Tree processor to scan and register HTML headings."""

    name = "autorefs-headings"

    def __init__(self, md: Markdown) -> None:
        super().__init__(md)

    def run(self, root: Element) -> None:
        """Run the tree processor."""
        if context := ContextPreprocessor.from_markdown(self.md):
            store = get_autorefs_store()
            self._scan_headings(root, store, context.page)

    def _scan_headings(
        self, parent: Element, store: AutorefsStore, page: Page
    ) -> None:
        for el in parent:
            if el.tag in HTAGS:
                if h_id := el.get("id"):
                    store.register_anchor(
                        page,
                        h_id,
                        title=el.text,
                    )
            else:
                self._scan_headings(el, store, page)


class AutorefsExtension(Extension):
    """Extension that transforms unresolved references into auto-references.

    Auto-references are resolved later, on the Rust side.
    """

    name = "zensical.extensions.autorefs"

    def extendMarkdown(self, md: Markdown) -> None:
        """Register the Markdown extension."""
        md.registerExtension(self)

        inline_processor = AutorefsInlineProcessor(md)
        md.inlinePatterns.register(
            inline_processor,
            AutorefsInlineProcessor.name,
            168,  # after markdown.inlinepatterns.ReferenceInlineProcessor
        )

        headings_treeprocessor = AutorefsHeadingsTreeprocessor(md)
        md.treeprocessors.register(
            headings_treeprocessor,
            AutorefsHeadingsTreeprocessor.name,
            0,
        )

        anchors_treeprocessor = AutorefsAnchorsTreeprocessor(md)
        md.treeprocessors.register(
            anchors_treeprocessor,
            AutorefsAnchorsTreeprocessor.name,
            0,
        )


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def get_autorefs_store() -> AutorefsStore:
    """Get the global autorefs instance."""
    global AUTOREFS  # noqa: PLW0603
    if AUTOREFS is None:
        AUTOREFS = AutorefsStore()
    return AUTOREFS


def get_autorefs_data() -> dict[str, Any]:
    """Get autorefs data.

    This function is called from Rust to replace the `<autoref>`
    elements written in the HTML output by both the autorefs
    Markdown extension (for manual cross-references) and the
    mkdocstrings extension (for automatic cross-references).
    """
    if AUTOREFS:
        return {
            "primary": AUTOREFS._primary_url_map,
            "secondary": AUTOREFS._secondary_url_map,
            "inventory": AUTOREFS._abs_url_map,
            "titles": AUTOREFS._title_map,
        }
    return {}


def set_autorefs_page(page: Page) -> None:
    """Set autorefs current page."""
    store = get_autorefs_store()
    store.current_page = page


def reset() -> None:
    """Reset global state in-between rebuilds."""
    global AUTOREFS  # noqa: PLW0603
    AUTOREFS = None


def makeExtension(**kwargs: Any) -> AutorefsExtension:
    """Register Markdown extension."""
    return AutorefsExtension(**kwargs)
