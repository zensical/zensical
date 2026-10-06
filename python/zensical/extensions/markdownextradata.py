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

import json
import logging
from copy import deepcopy
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, Any

import jinja2
import yaml
from markdown import Extension
from markdown.preprocessors import Preprocessor

from zensical.extensions.context import ContextPreprocessor

if TYPE_CHECKING:
    from collections.abc import Iterator

    from markdown import Markdown


_LOGGER = logging.getLogger(__name__)


@dataclass
class MarkdownExtraDataConfig:
    """Configuration for the markdownextradata Markdown extension."""

    data: str | None = None
    jinja_options: dict[str, Any] = field(default_factory=dict)


class MarkdownExtraDataPreprocessor(Preprocessor):
    """Build Jinja2 context, render body and title."""

    name = "markdownextradata"

    def __init__(self, md: Markdown, config: MarkdownExtraDataConfig) -> None:
        super().__init__(md)
        self.config = config
        self.env = jinja2.Environment(  # noqa: S701
            undefined=jinja2.DebugUndefined, **config.jinja_options
        )

    def run(self, lines: list[str]) -> list[str]:
        """Render body and title as Jinja2 templates with built context."""
        context = ContextPreprocessor.from_markdown(self.md)
        project_config = context.config if context else {}
        extra = load_extra_data(project_config, self.config.data)
        variables = {**project_config, "extra": extra, **extra}

        def render(source: str) -> str:
            try:
                return self.env.from_string(source).render(variables)
            except jinja2.TemplateSyntaxError:
                _LOGGER.exception(
                    "markdownextradata: Data names might not comply "
                    "with Python's variable naming conventions. "
                    "Try accessing the variable through the 'extra' dictionary."
                )
                raise

        # The effective title is rendered before the body, as in the plugin.
        page = context.page if context else None
        if page and isinstance(page.title, str):
            title = page.title
            page.title = render(title)

            # Source metadata is retained for other Markdown extensions.
            metadata_title = page.meta.get("title")
            if isinstance(metadata_title, str):
                page.rendered_metadata_title = (
                    page.title
                    if metadata_title == title
                    else render(metadata_title)
                )

        return render("\n".join(lines)).split("\n")


class MarkdownExtraDataExtension(Extension):
    """Compatibility adapter for mkdocs-markdownextradata-plugin."""

    name = "zensical.extensions.markdownextradata"

    def __init__(self, **kwargs: Any) -> None:
        """Initialize the extension."""
        self._enabled: bool = kwargs.pop("enabled", True)
        self._config = MarkdownExtraDataConfig(**kwargs)
        super().__init__()

    def extendMarkdown(self, md: Markdown) -> None:
        """Register Markdown extension."""
        if not self._enabled:
            return
        md.registerExtension(self)
        preprocessor = MarkdownExtraDataPreprocessor(md, self._config)
        # Jinja is rendered before fenced code and raw HTML are processed.
        md.preprocessors.register(preprocessor, preprocessor.name, 35)


def iter_data_files(
    config: dict[str, Any], data: str | None = None
) -> Iterator[tuple[Path, tuple[str, ...]]]:
    """Yield data files within the project root with relative namespaces."""
    root = Path(config.get("root_dir", ".")).resolve()
    directories = (
        [
            root / directory.strip()
            for directory in data.split(",")
            if directory.strip()
        ]
        if data
        else [root / "_data", root / config.get("docs_dir", "docs") / "_data"]
    )
    for data_directory in directories:
        directory = data_directory.resolve()
        if not directory.is_relative_to(root) or not directory.is_dir():
            continue

        # Extension order and later-directory precedence match version 0.2.6.
        for suffix in ("yaml", "yml", "json"):
            for path in directory.glob(f"**/*.{suffix}"):
                # Files reached through symlinks are checked before loading.
                if path.resolve().is_relative_to(root) and path.is_file():
                    yield (
                        path,
                        path.relative_to(directory).with_suffix("").parts,
                    )


def load_extra_data(
    config: dict[str, Any],
    data: str | None = None,
    *,
    for_templates: bool = False,
) -> dict[str, Any]:
    """Load data files into a copy of the configured extra variables.

    Convert YAML types to native configuration values for theme templates when
    `for_templates` is true. Otherwise, retain the original types for Markdown.
    """
    extra = deepcopy(config.get("extra") or {})
    for path, namespace in iter_data_files(config, data):
        contents = path.read_bytes()
        value = (
            json.loads(contents)
            if path.suffix == ".json"
            else yaml.safe_load(contents)
        )
        holder = extra
        for name in namespace[:-1]:
            holder = holder.setdefault(name, {})
        holder[namespace[-1]] = (
            _template_value(value) if for_templates else value
        )
    return extra


def _template_value(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(key): _template_value(item) for key, item in value.items()}
    if isinstance(value, (list, tuple, set)):
        return [_template_value(item) for item in value]
    if value is None or isinstance(value, (str, bool, int, float)):
        return value
    return str(value)


def makeExtension(**kwargs: Any) -> MarkdownExtraDataExtension:
    """Register Markdown extension."""
    return MarkdownExtraDataExtension(**kwargs)
