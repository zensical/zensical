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

import ast
import logging
import re
from dataclasses import dataclass, field
from functools import cache, wraps
from inspect import signature
from pathlib import Path
from textwrap import indent
from typing import TYPE_CHECKING, Any

import yaml
from markdown import Extension
from markdown.preprocessors import Preprocessor

from zensical.extensions.context import ContextPreprocessor

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable
    from typing import NoReturn

    from markdown import Markdown
    from pandas import DataFrame


# -----------------------------------------------------------------------------
# Constants
# -----------------------------------------------------------------------------


TABLE_READERS = (
    "read_csv",
    "read_table",
    "read_fwf",
    "read_excel",
    "read_yaml",
    "read_json",
    "read_feather",
    "read_raw",
)
"""Readers exposed by the standalone table-reader plugin."""

_PANDAS_FORMATS = (
    "csv",
    "table",
    "fwf",
    "excel",
    "yaml",
    "json",
    "feather",
)

_TAG_RE = re.compile(
    r"(?P<indent> *)"
    r"\{\{\s*(?P<reader>read_(?:csv|table|fwf|excel|yaml|json|feather|raw))"
    r"\((?P<arguments>.*?)\)\s*\}\}",
    flags=re.IGNORECASE,
)

_LOGGER = logging.getLogger(__name__)


# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


@dataclass(frozen=True)
class TableReaderConfig:
    """Configuration for the table-reader Markdown extension."""

    data_path: str = "."
    allow_missing_files: bool = False
    select_readers: list[str] = field(
        default_factory=lambda: list(TABLE_READERS)
    )

    def __post_init__(self) -> None:
        unknown = set(self.select_readers) - set(TABLE_READERS)
        if unknown:
            reader = sorted(unknown)[0]
            raise ValueError(f"unknown table-reader reader: {reader}")


@dataclass(frozen=True)
class _ReaderContext:
    """File resolution context shared by all table readers."""

    project_root: Path
    docs_dir: str | Path = "docs"
    page_path: str | Path | None = None
    data_path: str | Path = "."
    allow_missing_files: bool = False

    def candidates(self, filepath: str | Path) -> list[Path]:
        """Return safe candidate paths in table-reader search order."""
        project_root = self.project_root.resolve()
        input_path = Path(filepath)
        docs_dir = Path(self.docs_dir)
        if not docs_dir.is_absolute():
            docs_dir = project_root / docs_dir

        if input_path.is_absolute():
            candidates = [input_path]
        else:
            candidates = [
                project_root / self.data_path / input_path,
                docs_dir / self.data_path / input_path,
            ]
            if self.page_path is not None:
                page_path = Path(self.page_path)
                if not page_path.is_absolute():
                    page_path = docs_dir / page_path
                candidates.append(page_path.parent / input_path)

        result = []
        seen = set()
        for unresolved in candidates:
            candidate = unresolved.resolve()
            if not candidate.is_relative_to(project_root):
                raise ValueError(
                    f"Table file must be within project root: {filepath}"
                )
            if candidate not in seen:
                seen.add(candidate)
                result.append(candidate)
        return result

    def resolve(self, filepath: str | Path) -> Path | None:
        """Resolve a table path or handle it as configured when missing."""
        candidates = self.candidates(filepath)
        for candidate in candidates:
            if candidate.is_file():
                return candidate

        searched = ", ".join(str(path) for path in candidates)
        message = (
            f"[table-reader]: Cannot find table file '{filepath}'. "
            f"The following paths were searched: {searched}"
        )
        if self.allow_missing_files:
            _LOGGER.warning(message)
            return None
        raise FileNotFoundError(message)


class TableReaderPreprocessor(Preprocessor):
    """Replace table-reader tags without evaluating general Jinja syntax."""

    name = "table_reader"

    def __init__(self, md: Markdown, config: TableReaderConfig) -> None:
        super().__init__(md)
        self.config = config

    def run(self, lines: list[str]) -> list[str]:
        """Replace selected table-reader calls with their file contents."""
        text = "\n".join(lines)
        selected = set(self.config.select_readers)
        if not any(
            match.group("reader").lower() in selected
            for match in _TAG_RE.finditer(text)
        ):
            return lines

        context = ContextPreprocessor.from_markdown(self.md)
        page = context.page if context else None
        project_config = context.config if context else {}
        readers = _get_table_readers(
            Path(project_config.get("root_dir", ".")),
            docs_dir=project_config.get("docs_dir", "docs"),
            page_path=page.path if page else None,
            data_path=self.config.data_path,
            allow_missing_files=self.config.allow_missing_files,
        )

        def replace(match: re.Match[str]) -> str:
            reader_name = match.group("reader").lower()
            if reader_name not in selected:
                return match.group(0)
            args, kwargs = _parse_arguments(match.group("arguments"))
            result = readers[reader_name](*args, **kwargs)
            return _fix_indentation(result, match.group("indent"))

        text = _TAG_RE.sub(replace, text)
        return text.split("\n")


class TableReaderExtension(Extension):
    """Compatibility adapter for mkdocs-table-reader-plugin."""

    name = "zensical.extensions.table_reader"

    def __init__(self, **kwargs: Any) -> None:
        """Initialize the extension."""
        self._enabled: bool = kwargs.pop("enabled", True)
        self._kwargs: dict[str, Any] = kwargs

    def extendMarkdown(self, md: Markdown) -> None:
        """Register the table-reader preprocessor."""
        if not self._enabled:
            return
        md.registerExtension(self)
        config = TableReaderConfig(**self._kwargs)
        preprocessor = TableReaderPreprocessor(md, config)
        # Before pymdownx.superfences parses fenced content.
        md.preprocessors.register(preprocessor, preprocessor.name, 35)


# -----------------------------------------------------------------------------
# Extension entry point
# -----------------------------------------------------------------------------


def makeExtension(**kwargs: Any) -> TableReaderExtension:
    """Register Markdown extension."""
    return TableReaderExtension(**kwargs)


# -----------------------------------------------------------------------------
# Standalone tag parsing
# -----------------------------------------------------------------------------


def _parse_arguments(source: str) -> tuple[list[Any], dict[str, Any]]:
    """Parse only literal positional and keyword arguments from a call."""
    try:
        return _literal_arguments(source)
    except (SyntaxError, TypeError, ValueError) as error:
        raise ValueError(
            f"Could not safely parse table-reader arguments: {source}"
        ) from error


def _literal_arguments(source: str) -> tuple[list[Any], dict[str, Any]]:
    """Parse a synthetic function call into literal arguments."""
    expression = ast.parse(f"_reader({source})", mode="eval").body
    if not isinstance(expression, ast.Call):
        raise TypeError("table-reader tag must contain a function call")
    args = [ast.literal_eval(arg) for arg in expression.args]
    kwargs = {}
    for keyword in expression.keywords:
        if keyword.arg is None:
            raise TypeError("table-reader does not support keyword expansion")
        kwargs[keyword.arg] = ast.literal_eval(keyword.value)
    return args, kwargs


def _fix_indentation(text: str, leading_spaces: str) -> str:
    """Apply the indentation behavior of the standalone MkDocs plugin."""
    prefix = " " * (len(leading_spaces) // 4)
    return "\n".join(indent(line, prefix) for line in text.split("\n"))


# -----------------------------------------------------------------------------
# Shared filters and readers
# -----------------------------------------------------------------------------


def _pandas_read_yaml(
    func: Callable[..., DataFrame],
) -> Callable[..., DataFrame]:
    @wraps(func)
    def inner(filepath: str | Path, **kwargs: Any) -> DataFrame:
        with open(filepath, encoding="utf8") as file:
            return func(yaml.safe_load(file), **kwargs)

    return inner


def _add_indentation(text: str, *, spaces: int = 0, tabs: int = 0) -> str:
    """Indent text using spaces or tabs."""
    if spaces and tabs:
        raise ValueError(
            "You can only specify either spaces or tabs, not both."
        )
    if spaces:
        prefix = " " * spaces
    elif tabs:
        prefix = "\t" * tabs
    else:
        return text

    return "\n".join(indent(line, prefix) for line in text.split("\n"))


def _convert_to_md_table(df: DataFrame, **kwargs: Any) -> str:
    """Convert a pandas dataframe to a Markdown table."""

    def escape_pipes(text: str) -> str:
        return re.sub(r"(?<!\\)\|", "\\|", text)

    df.columns = [
        escape_pipes(c) if isinstance(c, str) else c for c in df.columns
    ]
    df = df.map(lambda s: escape_pipes(s) if isinstance(s, str) else s)
    kwargs.setdefault("index", False)
    kwargs.setdefault("tablefmt", "pipe")
    result = df.to_markdown(**kwargs)
    if result is None:
        raise ValueError("Markdown table conversion produced no output")
    return result


def _param_names(func: Callable) -> list[str]:
    return [
        param.name
        for param in signature(func).parameters.values()
        if param.kind not in (param.VAR_POSITIONAL, param.VAR_KEYWORD)
    ]


def _filter_kwargs(
    kwargs: dict[str, Any], param_names: Iterable[str]
) -> tuple[dict[str, Any], dict[str, Any]]:
    into, not_into = {}, {}
    for key, value in kwargs.items():
        if key in param_names:
            into[key] = value
        else:
            not_into[key] = value
    return into, not_into


def _missing(filepath: str | Path) -> str:
    """Return the placeholder used when missing files are allowed."""
    return f"{{{{ Cannot find '{filepath}' }}}}"


def _extract_filepath(
    args: tuple[Any, ...], kwargs: dict[str, Any]
) -> tuple[str | Path, tuple[Any, ...], dict[str, Any]]:
    """Extract the first argument using table-reader's supported spellings."""
    if args:
        filepath, *remaining = args
        return filepath, tuple(remaining), kwargs
    if "filepath_or_buffer" in kwargs:
        kwargs = dict(kwargs)
        filepath = kwargs.pop("filepath_or_buffer")
        return filepath, (), kwargs
    raise TypeError("table reader is missing its required file path")


def _relative_pandas_reader(
    context: _ReaderContext, func: Callable[..., DataFrame]
) -> Callable[..., DataFrame | str]:
    @wraps(func)
    def inner(*args: Any, **kwargs: Any) -> DataFrame | str:
        filepath, args, kwargs = _extract_filepath(args, kwargs)
        if not isinstance(filepath, (str, Path)):
            raise TypeError(
                f"Only str and Path are supported in pd_{func.__name__}"  # ty:ignore[unresolved-attribute]
            )
        resolved = context.resolve(filepath)
        if resolved is None:
            return _missing(filepath)
        return func(resolved, *args, **kwargs)

    return inner


def _relative_reader(
    reader: Callable[..., DataFrame | str],
) -> Callable[..., str]:
    reader_params = _param_names(reader)

    def inner(*args: Any, **kwargs: Any) -> str:
        filepath, args, kwargs = _extract_filepath(args, kwargs)
        read_args, write_args = _filter_kwargs(kwargs, reader_params)
        dataframe = reader(filepath, *args, **read_args)
        if isinstance(dataframe, str):
            return dataframe
        return _convert_to_md_table(dataframe, **write_args)

    inner.__doc__ = (
        "Read data using pandas and convert it to a Markdown table. "
        "Keyword arguments are split and passed to the relevant pandas reader "
        "as well as dataframes' `to_markdown()` method."
    )
    return inner


def _relative_raw_reader(context: _ReaderContext) -> Callable[..., str]:
    def inner(*args: Any, **kwargs: Any) -> str:
        filepath, _, _ = _extract_filepath(args, kwargs)
        if not isinstance(filepath, (str, Path)):
            raise TypeError("Only str and Path are supported in read_raw")
        resolved = context.resolve(filepath)
        if resolved is None:
            return _missing(filepath)
        return resolved.read_text(encoding="utf-8")

    return inner


def _get_table_readers(
    project_root: Path,
    *,
    docs_dir: str | Path = "docs",
    page_path: str | Path | None = None,
    data_path: str | Path = ".",
    allow_missing_files: bool = False,
) -> dict[str, Callable]:
    """Build table readers for one project and page context."""
    context = _ReaderContext(
        project_root=project_root,
        docs_dir=docs_dir,
        page_path=page_path,
        data_path=data_path,
        allow_missing_files=allow_missing_files,
    )
    readers: dict[str, Callable] = {"read_raw": _relative_raw_reader(context)}
    for name, pandas_reader in _get_pandas_readers().items():
        reader = _relative_pandas_reader(context, pandas_reader)
        readers[name] = reader
        readers[name.removeprefix("pd_")] = _relative_reader(reader)
    return readers


@cache
def _get_pandas_readers() -> dict[str, Callable]:
    """Return the pandas reader implementations or dependency placeholders."""
    try:
        import pandas  # noqa: PLC0415
        import tabulate  # noqa: F401,PLC0415
    except ImportError:
        return {
            name: reader
            for name, reader in _get_fake_table_readers().items()
            if name.startswith("pd_read_")
        }
    return {
        "pd_read_csv": pandas.read_csv,
        "pd_read_table": pandas.read_table,
        "pd_read_fwf": pandas.read_fwf,
        "pd_read_excel": pandas.read_excel,
        "pd_read_yaml": _pandas_read_yaml(pandas.json_normalize),
        "pd_read_json": pandas.read_json,
        "pd_read_feather": pandas.read_feather,
    }


@cache
def _get_fake_table_readers() -> dict[str, Callable]:
    """Build placeholder readers used without pandas and tabulate."""
    message = "table reading requires pandas and tabulate packages"

    def raiser() -> Callable[..., NoReturn]:
        def inner(*args: Any, **kwargs: Any) -> NoReturn:  # noqa: ARG001
            raise RuntimeError(message)

        return inner

    readers = {}
    for fmt in _PANDAS_FORMATS:
        readers[f"pd_read_{fmt}"] = raiser()
        readers[f"read_{fmt}"] = raiser()
    return readers
