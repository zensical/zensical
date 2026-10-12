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
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Any
from urllib.parse import urljoin

from markdown import Extension
from markdown.preprocessors import Preprocessor
from pymdownx.snippets import SnippetExtension, SnippetPreprocessor

from zensical.extensions.context import ContextPreprocessor

if TYPE_CHECKING:
    from markdown import Markdown


@dataclass
class IncludeMarkdownConfig:
    """Configure the include-markdown tag adapter."""

    opening_tag: str = "{%"
    closing_tag: str = "%}"
    encoding: str = "utf-8"
    preserve_includer_indent: bool = True
    dedent: bool = False
    include_from_url: bool = True
    directives: dict[str, str] = field(default_factory=dict)

    def __post_init__(self) -> None:
        for option in ("opening_tag", "closing_tag"):
            value = getattr(self, option)
            if not isinstance(value, str) or not value:
                raise ValueError(
                    f"include-markdown {option} must be a nonempty string"
                )
        if not isinstance(self.directives, dict):
            raise TypeError("include-markdown directives must be a mapping")
        for name, value in self.directives.items():
            if name not in {"include", "include-markdown"}:
                raise ValueError(f"unknown include-markdown directive: {name}")
            if (
                not isinstance(value, str)
                or not value
                or any(character.isspace() for character in value)
            ):
                raise ValueError(
                    "include-markdown directives must contain "
                    "nonempty names without whitespace"
                )


_QUOTED = r"""(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')"""
_OPTIONS = re.compile(rf"\s+(?P<name>[a-z][a-z-]*)\s*=\s*(?:{_QUOTED}|[^\s]+)")
_OPTION_NAMES = {
    "start",
    "end",
    "preserve-includer-indent",
    "dedent",
    "exclude",
    "trailing-newlines",
    "recursive",
    "order",
    "encoding",
    "rewrite-relative-urls",
    "comments",
    "heading-offset",
}


class IncludeMarkdownPreprocessor(SnippetPreprocessor):
    """Translate include tags before Snippets reads each document."""

    def __init__(
        self,
        md: Markdown,
        config: IncludeMarkdownConfig,
        snippets: dict[str, Any],
    ) -> None:
        super().__init__(snippets, md)
        self.md = md
        self.config = config
        names = {
            "include": "include",
            "include-markdown": "include-markdown",
            **config.directives,
        }
        directives = "|".join(
            re.escape(name) for name in sorted(set(names.values()))
        )
        self.tags = re.compile(
            rf"{re.escape(config.opening_tag)}\s*(?:{directives})(?=\s)"
            rf"(?P<body>(?:{_QUOTED}|(?!{re.escape(config.closing_tag)})[^\"'])*?)"
            rf"{re.escape(config.closing_tag)}[ \t]*",
            re.DOTALL,
        )
        self.arguments = re.compile(
            rf"\s*(?P<path>{_QUOTED})(?P<options>.*)", re.DOTALL
        )

    def translate_tags(
        self,
        lines: list[str],
        file_name: str | None = None,
        is_url: bool = False,  # noqa: FBT002
    ) -> list[str]:
        """Translate include tags into standalone native Snippets directives.

        Leave file reading and content processing to Snippets so converted
        documents produce the same output after users remove this adapter.
        """
        context = ContextPreprocessor.from_markdown(self.md)
        project = context.config if context else {}
        docs_dir = Path(project.get("root_dir", ".")) / project.get(
            "docs_dir", "docs"
        )
        page_path = context.page.path if context else "index.md"
        source = file_name or str(docs_dir / page_path)
        text = "\n".join(lines)

        def replace(match: re.Match[str]) -> str:
            arguments = self.arguments.fullmatch(match["body"])
            if arguments is None:
                raise ValueError("include-markdown requires a quoted file path")

            # Only known literal options are accepted; their values are ignored.
            options = arguments["options"].rstrip()
            position = 0
            while position < len(options):
                option = _OPTIONS.match(options, position)
                if option is None or option["name"] not in _OPTION_NAMES:
                    raise ValueError(
                        f"invalid include-markdown options: {options}"
                    )
                position = option.end()

            quoted = arguments["path"]
            path = (
                quoted[1:-1]
                .replace("\\" + quoted[0], quoted[0])
                .replace("\\\\", "\\")
            )
            if not path or "\n" in path or "\r" in path:
                raise ValueError(
                    "include-markdown requires a nonempty, "
                    "single-line file path"
                )
            if is_url:
                path = urljoin(source, path)
            elif not path.lower().startswith(("https://", "http://")):
                base = (
                    Path(source).parent
                    if path.startswith(("./", "../"))
                    else docs_dir
                )
                path = str((base / path).resolve())

            # Each directive must be placed on its own line for Snippets.
            line_start = text.rfind("\n", 0, match.start()) + 1
            line_end = text.find("\n", match.end())
            if line_end < 0:
                line_end = len(text)
            indent = text[line_start : match.start()]
            if indent.strip() or text[match.end() : line_end].strip():
                raise ValueError(
                    "include-markdown tags must be on their own lines"
                )
            if not self.config.preserve_includer_indent:
                indent = ""

            # Quotes in paths are preserved by block notation. Dedenting is
            # applied to the whole file, and colon suffixes are kept literal.
            return f"--8<--\n{indent}{path}:1\n--8<--"

        return self.tags.sub(replace, text).split("\n")

    def parse_snippets(
        self,
        lines: list[str],
        file_name: str | None = None,
        is_url: bool = False,  # noqa: FBT002
        is_section: bool = False,  # noqa: FBT002
    ) -> list[str]:
        """Translate nested tags and delegate file inclusion to Snippets."""
        return super().parse_snippets(
            self.translate_tags(lines, file_name, is_url),
            file_name,
            is_url,
            is_section,
        )


class IncludeMarkdownTagPreprocessor(Preprocessor):
    """Translate page tags before macros without expanding included files."""

    def __init__(
        self, md: Markdown, snippets: IncludeMarkdownPreprocessor
    ) -> None:
        super().__init__(md)
        self.snippets = snippets

    def run(self, lines: list[str]) -> list[str]:
        """Translate page tags and leave file expansion to native Snippets."""
        return self.snippets.translate_tags(lines)


class IncludeMarkdownExtension(Extension):
    """Adapt include-markdown 7.3.0 syntax to PyMdown Snippets."""

    name = "zensical.extensions.include_markdown"

    def __init__(self, **kwargs: Any) -> None:
        self._enabled = kwargs.pop("enabled", True)
        self._config = IncludeMarkdownConfig(**kwargs)
        super().__init__()

    def extendMarkdown(self, md: Markdown) -> None:
        """Register a Snippets preprocessor with include-tag translation."""
        if not self._enabled:
            return
        snippets = next(
            (
                extension
                for extension in md.registeredExtensions
                if isinstance(extension, SnippetExtension)
            ),
            None,
        )
        if snippets is None:
            snippets = SnippetExtension(
                base_path=["."],
                restrict_base_path=False,
                check_paths=True,
                encoding=self._config.encoding,
                url_download=self._config.include_from_url,
                dedent_subsections=self._config.dedent,
            )
        self._processor = IncludeMarkdownPreprocessor(
            md, self._config, snippets.getConfigs()
        )
        md.registerExtension(self)
        # PyMdown's "snippet" preprocessor is replaced with this subclass.
        # Native snippet processing is retained, and include tags are translated
        # during recursive inclusion. A second processing pass and duplicate
        # auto_append content are avoided by reusing the name. Registration must
        # follow Snippets.
        #
        # Page tags are translated before macros so include tags are not parsed
        # as Jinja statements. File expansion is kept at Snippets' native
        # priority (32), after macros, so conversion to native notation
        # preserves the order of Markdown processing.
        md.preprocessors.register(
            IncludeMarkdownTagPreprocessor(md, self._processor),
            "include_markdown",
            36,
        )
        md.preprocessors.register(self._processor, "snippet", 32)

    def reset(self) -> None:
        """Clear downloaded snippets between Markdown conversions."""
        self._processor.download.cache_clear()


def makeExtension(**kwargs: Any) -> IncludeMarkdownExtension:
    """Create the include-markdown compatibility extension."""
    return IncludeMarkdownExtension(**kwargs)
