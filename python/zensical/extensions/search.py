# Copyright (c) 2025 Zensical and contributors

# SPDX-License-Identifier: MIT
# Third-party contributions licensed under DCO

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

from html import escape
from html.parser import HTMLParser

from markdown import Extension
from markdown.postprocessors import Postprocessor

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class SearchProcessor(Postprocessor):
    """
    Post processor that extracts searchable content from the rendered HTML.
    """

    def __init__(self, md):
        super().__init__(md)
        self.data = []

    def run(self, html):
        """Process the rendered HTML and extract text length."""

        # Divide page content into sections
        parser = Parser()
        parser.feed(html)
        parser.close()

        # Extract data from sections that are not excluded
        self.data = []
        for section in parser.data:
            if not section.is_excluded():
                # Compute title and text
                title = "".join(section.title).strip()
                text = "".join(section.text).strip()

                # Store data for external access
                self.data.append(
                    {
                        "location": section.id,
                        "level": section.level,
                        "title": title,
                        "text": text,
                        "path": [],
                        "tags": [],
                    }
                )

        # Return the original HTML unchanged
        return html


class SearchExtension(Extension):
    """Markdown extension for search indexing."""

    def __init__(self, **kwargs):
        self.config = {"keep": [set(), "Set of HTML tags to keep in output"]}
        super().__init__(**kwargs)

    def extendMarkdown(self, md):
        """Register the PostProcessor with Markdown."""
        processor = SearchProcessor(md)
        md.postprocessors.register(processor, "search", 0)


def makeExtension(**kwargs):
    """Factory function for creating the extension."""
    return SearchExtension(**kwargs)


# -----------------------------------------------------------------------------


# HTML element
class Element:
    """
    An element with attributes, essentially a small wrapper object for the
    parser to access attributes in other callbacks than handle_starttag.
    """

    # Initialize HTML element
    def __init__(self, tag, attrs=None):
        self.tag = tag
        self.attrs = attrs or {}

    # String representation
    def __repr__(self):
        return self.tag

    # Support comparison (compare by tag only)
    def __eq__(self, other):
        if other is Element:
            return self.tag == other.tag
        else:
            return self.tag == other

    # Support set operations
    def __hash__(self):
        return hash(self.tag)

    # Check whether the element should be excluded
    def is_excluded(self):
        return "data-search-exclude" in self.attrs


# -----------------------------------------------------------------------------


# HTML section
class Section:
    """
    A block of text with markup, preceded by a title (with markup), i.e., a
    headline with a certain level (h1-h6). Internally used by the parser.
    """

    # Initialize HTML section
    def __init__(self, el, level, depth=0):
        self.el = el
        self.depth = depth
        self.level = level

        # Initialize section data
        self.text = []
        self.title = []
        self.id = None

    # String representation
    def __repr__(self):
        if self.id:
            return "#".join([self.el.tag, self.id])
        else:
            return self.el.tag

    # Check whether the section should be excluded
    def is_excluded(self):
        return self.el.is_excluded()


# -----------------------------------------------------------------------------


# HTML parser
class Parser(HTMLParser):
    """
    This parser divides the given string of HTML into a list of sections, each
    of which are preceded by a h1-h6 level heading. A white- and blacklist of
    tags dictates which tags should be preserved as part of the index, and
    which should be ignored in their entirety.
    """

    # Initialize HTML parser
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

        # Tags to skip
        self.skip = set(
            [
                "object",  # Objects
                "script",  # Scripts
                "style",  # Styles
            ]
        )

        # Current context and section
        self.context = []
        self.section = None

        # All parsed sections
        self.data = []

    # Called at the start of every HTML tag
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)

        # Ignore self-closing tags
        el = Element(tag, attrs)
        if tag not in void:
            self.context.append(el)
        else:
            return

        # Handle heading
        if tag in ([f"h{x}" for x in range(1, 7)]):
            depth = len(self.context)
            if "id" in attrs:
                # Ensure top-level section
                if tag != "h1" and not self.data:
                    self.section = Section(Element("hx"), 1, depth)
                    self.data.append(self.section)

                # Set identifier, if not first section
                self.section = Section(el, int(tag[1:2]), depth)
                if self.data:
                    self.section.id = attrs["id"]

                # Append section to list
                self.data.append(self.section)

        # Handle preface - ensure top-level section
        if not self.section:
            self.section = Section(Element("hx"), 1)
            self.data.append(self.section)

        # Handle special cases to skip
        for key, value in attrs.items():
            # Skip block if explicitly excluded from search
            if key == "data-search-exclude":
                self.skip.add(el)
                return

            # Skip line numbers - see https://bit.ly/3GvubZx
            if key == "class" and value == "linenodiv":
                self.skip.add(el)
                return

        # Render opening tag if kept
        if not self.skip.intersection(self.context) and tag in keep:
            # Check whether we're inside the section title
            data = self.section.text
            if self.section.el in self.context:
                data = self.section.title

            # Append to section title or text
            data.append(f"<{tag}>")

    # Called at the end of every HTML tag
    def handle_endtag(self, tag):
        if not self.context or self.context[-1] != tag:
            return

        # Check whether we're exiting the current context, which happens when
        # a headline is nested in another element. In that case, we close the
        # current section, continuing to append data to the previous section,
        # which could also be a nested section – see https://bit.ly/3IxxIJZ
        if self.section.depth > len(self.context):
            for section in reversed(self.data):
                if section.depth <= len(self.context):
                    # Set depth to infinity in order to denote that the current
                    # section is exited and must never be considered again.
                    self.section.depth = float("inf")
                    self.section = section
                    break

        # Remove element from skip list
        el = self.context.pop()
        if el in self.skip:
            if el.tag not in ["script", "style", "object"]:
                self.skip.remove(el)
            return

        # Render closing tag if kept
        if not self.skip.intersection(self.context) and tag in keep:
            # Check whether we're inside the section title
            data = self.section.text
            if self.section.el in self.context:
                data = self.section.title

            # Search for corresponding opening tag
            index = data.index(f"<{tag}>")
            for i in range(index + 1, len(data)):
                if not data[i].isspace():
                    index = len(data)
                    break

            # Remove element if empty (or only whitespace)
            if len(data) > index:
                while len(data) > index:
                    data.pop()

            # Append to section title or text
            else:
                data.append(f"</{tag}>")

    # Called for the text contents of each tag
    def handle_data(self, data):
        if self.skip.intersection(self.context):
            return

        # Collapse whitespace in non-pre contexts
        if "pre" not in self.context:
            if not data.isspace():
                data = data.replace("\n", " ")
            else:
                data = " "

        # Handle preface - ensure top-level section
        if not self.section:
            self.section = Section(Element("hx"), 1)
            self.data.append(self.section)

        # Handle section headline
        if self.section.el in self.context:
            permalink = False
            for el in self.context:
                if el.tag == "a" and el.attrs.get("class") == "headerlink":
                    permalink = True

            # Ignore permalinks
            if not permalink:
                self.section.title.append(escape(data, quote=False))

        # Collapse adjacent whitespace
        elif data.isspace():
            if not self.section.text or not self.section.text[-1].isspace():
                self.section.text.append(data)
            elif "pre" in self.context:
                self.section.text.append(data)

        # Handle everything else
        else:
            self.section.text.append(escape(data, quote=False))


# -----------------------------------------------------------------------------
# Data
# -----------------------------------------------------------------------------

# Tags to keep
keep = set(
    [
        "p",
        "code",
        "pre",
        "li",
        "ol",
        "ul",
        "sub",
        "sup",
    ]
)

# Tags that are self-closing
void = set(
    [
        "area",
        "base",
        "br",
        "col",
        "embed",
        "hr",
        "img",
        "input",
        "link",
        "meta",
        "param",
        "source",
        "track",
        "wbr",
    ]
)
