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
from typing import TYPE_CHECKING, Any, TypeAlias
from xml.etree.ElementTree import Comment, Element

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

if TYPE_CHECKING:
    from pathlib import Path
    from re import Match

    from markdown import Markdown

    from zensical.extensions.context import Page


# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------


# URL order determines which target wins when several are equally
# suitable. Dict keys give us that order plus constant-time lookups.
OrderedSet: TypeAlias = dict[str, None]
URLMap: TypeAlias = dict[str, OrderedSet]


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

        self._primary_url_map: URLMap = {}
        self._secondary_url_map: URLMap = {}
        self._abs_url_map: dict[str, str] = {}
        self._title_map: dict[str, str] = {}
        self._page_registrations: dict[str, set[tuple[bool, str, str]]] = {}

    def set_page(self, page: Page) -> None:
        """Set the current page and discard its previous registrations."""
        self.current_page = page
        self.pop_page(page.url)

    def pop_page(self, page_url: str) -> dict[str, Any]:
        """Remove and return registrations owned by one page."""
        registrations = self._page_registrations.pop(page_url, set())
        primary = self._pop_urls(self._primary_url_map, registrations, True)
        secondary = self._pop_urls(
            self._secondary_url_map, registrations, False
        )
        urls = {
            url
            for values in (primary, secondary)
            for entries in values.values()
            for url in entries
        }
        titles = {
            url: self._title_map.pop(url)
            for url in urls
            if url in self._title_map
        }
        return {
            "primary": primary,
            "secondary": secondary,
            "titles": titles,
        }

    @staticmethod
    def _pop_urls(
        url_map: URLMap,
        registrations: set[tuple[bool, str, str]],
        primary: bool,
    ) -> dict[str, list[str]]:
        """Remove registered URLs from one URL map, preserving their order."""
        selected: dict[str, set[str]] = {}
        for is_primary, identifier, url in registrations:
            if is_primary == primary:
                selected.setdefault(identifier, set()).add(url)

        result: dict[str, list[str]] = {}
        for identifier, selected_urls in selected.items():
            urls = url_map.get(identifier, {})
            result[identifier] = [url for url in urls if url in selected_urls]
            for url in selected_urls:
                urls.pop(url, None)
            if not urls:
                url_map.pop(identifier, None)
        return result

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
        url_map.setdefault(identifier, {})[url] = None
        self._page_registrations.setdefault(page.url, set()).add(
            (primary, identifier, url)
        )
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
    backlink_marker = "data-zensical-autoref"

    def __init__(
        self,
        *args: Any,
        record_backlinks: bool = False,
        **kwargs: Any,
    ) -> None:
        super().__init__(REFERENCE_RE, *args, **kwargs)
        self._record_backlinks = record_backlinks

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
        """Create a tag that can be resolved after site settlement."""
        el = Element("autoref")
        if self._record_backlinks:
            el.set(self.backlink_marker, "")
        if self.hook:
            identifier = self.hook.expand_identifier(identifier)
            el.attrib.update(self.hook.get_context().as_dict())
        el.set("identifier", identifier)
        el.text = text
        if slug:
            el.attrib["slug"] = slug
        return el


class BacklinkContextTreeProcessor(Treeprocessor):
    """Expose one Markdown conversion's backlink context to the Rust visitor.

    Mkdocstrings mutates ``initial_id`` around nested Markdown conversions. The
    two constant-time boundary markers preserve that context without walking
    the ElementTree in Python.
    """

    name = "mkdocs-autorefs-backlinks"
    initial_id: str | None = None

    def run(self, root: Element) -> None:
        """Bracket this rendered fragment with its initial heading ID."""
        start = "zensical:autoref-context:start"
        if self.initial_id:
            start = f"{start}:{self.initial_id.encode().hex()}"
        root.insert(0, Comment(start))
        root.append(Comment("zensical:autoref-context:end"))


class AutorefsExtension(Extension):
    """Extension that transforms unresolved references into auto-references.

    Auto-references are resolved later, on the Rust side.
    """

    name = "zensical.extensions.autorefs"

    def __init__(self, **kwargs: Any) -> None:
        """Initialize the extension."""
        self._enabled: bool = kwargs.pop("enabled", True)
        self._record_backlinks: bool = kwargs.pop("record_backlinks", False)

    def extendMarkdown(self, md: Markdown) -> None:
        """Register the Markdown extension."""
        if not self._enabled:
            return
        md.registerExtension(self)

        record_backlinks = self._record_backlinks and (
            "attr_list" in md.treeprocessors or "toc" in md.treeprocessors
        )
        inline_processor = AutorefsInlineProcessor(
            md,
            record_backlinks=record_backlinks,
        )
        md.inlinePatterns.register(
            inline_processor,
            AutorefsInlineProcessor.name,
            168,  # after markdown.inlinepatterns.ReferenceInlineProcessor
        )

        if record_backlinks:
            store = get_autorefs_store()
            store.record_backlinks = True
            processor = BacklinkContextTreeProcessor(md)
            md.treeprocessors.register(processor, processor.name, 0)


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def get_autorefs_store() -> AutorefsStore:
    """Get the global autorefs instance."""
    global AUTOREFS  # noqa: PLW0603
    if AUTOREFS is None:
        AUTOREFS = AutorefsStore()
    return AUTOREFS


def get_autorefs_page_data(page_url: str) -> dict[str, Any]:
    """Take page-local autorefs data.

    Rust combines these registrations into the settled URL registry used to
    resolve `<autoref>` elements emitted by autorefs and mkdocstrings.
    """
    if AUTOREFS:
        return AUTOREFS.pop_page(page_url)
    return {"primary": {}, "secondary": {}, "titles": {}}


def get_autorefs_inventory_data() -> dict[str, str] | None:
    """Return global inventory URLs if Markdown rendering initialized them."""
    if AUTOREFS is None:
        return None
    return AUTOREFS._abs_url_map


def set_autorefs_page(page: Page) -> None:
    """Set autorefs current page."""
    store = get_autorefs_store()
    store.set_page(page)


def reset() -> None:
    """Reset global state in-between rebuilds."""
    global AUTOREFS  # noqa: PLW0603
    AUTOREFS = None


def makeExtension(**kwargs: Any) -> AutorefsExtension:
    """Register Markdown extension."""
    return AutorefsExtension(**kwargs)
