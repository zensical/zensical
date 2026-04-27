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

import importlib.util
import inspect
import platform
import subprocess
import traceback
from collections.abc import Callable, Iterable
from dataclasses import asdict, dataclass, field
from datetime import datetime
from functools import cache
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal, TypeAlias
from urllib.parse import urlparse

import jinja2
import yaml
from jinja2.exceptions import UndefinedError
from markdown import Extension
from markdown.preprocessors import Preprocessor

from zensical.extensions.context import ContextPreprocessor

if TYPE_CHECKING:
    from jinja2 import Environment
    from markdown import Markdown


# -----------------------------------------------------------------------------
# Constants
# -----------------------------------------------------------------------------


VariablesType: TypeAlias = dict[str, Any]
MacrosType: TypeAlias = dict[str, Callable[..., Any]]
FiltersType: TypeAlias = dict[str, Callable[..., Any]]
VariablesMacrosFiltersType: TypeAlias = tuple[
    VariablesType, MacrosType, FiltersType
]

_MACROS_INFO = """
{#
Template for the macro_info() command
(C) Laurent Franceschetti 2019
#}

## Macros Plugin Environment

### General List

All available variables and filters within the macros plugin:

{{ context() | pretty }}

### Config Information

Standard configuration information. Do not try to modify.

e.g. {{ "`{{ config.docs_dir }}`" }}

See also the [MkDocs documentation on the config object](https://www.MkDocs.org/user-guide/custom-themes/#config).

{{ context(config)| pretty }}

### Macros

These macros have been defined programmatically for this environment
(module or pluglets).

{{ context(macros)| pretty }}

### Git Information

Information available on the last commit and the git repository containing the
documentation project:

e.g. {{ "`{{ git.message }}`" }}

{{ context(git)| pretty }}

### Page Attributes

Provided by MkDocs. These attributes change for every page
(the attributes shown are for this page).

e.g. {{ "`{{ page.title }}`" }}

See also the [MkDocs documentation on the page object](https://www.MkDocs.org/user-guide/custom-themes/#page).

{{ context(page)| pretty }}

To have all titles of all pages, use:

    {% raw %}
    {% for page in navigation.pages %}
    - {{ page.title }}
    {% endfor %}
    {% endraw %}


### Plugin Filters

These filters are provided as a standard by the macros plugin.

{{ context(filters)| pretty }}

### Builtin Jinja2 Filters

These filters are provided by Jinja2 as a standard.

See also the [Jinja2 documentation on builtin filters](https://jinja.palletsprojects.com/en/3.1.x/templates/#builtin-filters).

{{ context(filters_builtin) | pretty }}
"""


# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class MacroEnv:
    """Minimal env object for compatibility with MkDocs Macros."""

    def __init__(self) -> None:
        self.variables: VariablesType = {}
        self.macros: MacrosType = {}
        self.filters: FiltersType = {}

    def macro(
        self, fn: Callable[..., Any] | None = None, name: str | None = None
    ) -> Any:
        """Register a macro.

        Use as `@env.macro` or `env.macro(func)` or `env.macro(func, 'name')`.
        """
        if fn is None:
            return lambda f: self.macro(f, name)
        self.macros[name or fn.__name__] = fn  # ty:ignore[unresolved-attribute]
        return fn

    def filter(
        self, fn: Callable[..., Any] | None = None, name: str | None = None
    ) -> Any:
        """Register a filter.

        Use as `@env.filter` or `env.filter(func)`.
        """
        if fn is None:
            return lambda f: self.filter(f, name)
        self.filters[name or fn.__name__] = fn  # ty:ignore[unresolved-attribute]
        return fn


@dataclass
class MacrosConfig:
    """Configuration for the macros Markdown extension."""

    module_name: str = "main"
    modules: list[str] = field(default_factory=list)
    include_yaml: list[str] | dict[str, str] = field(default_factory=list)
    include_dir: str = ""
    render_by_default: bool = True
    on_error_fail: bool = False
    on_undefined: Literal["keep", "strict"] = "keep"
    verbose: bool = False
    j2_block_start_string: str = "{%"
    j2_block_end_string: str = "%}"
    j2_variable_start_string: str = "{{"
    j2_variable_end_string: str = "}}"
    j2_comment_start_string: str = "{#"
    j2_comment_end_string: str = "#}"
    j2_extensions: list[str] = field(default_factory=list)


class MacrosPreprocessor(Preprocessor):
    """Build Jinja2 context, render body."""

    name = "macros"

    def __init__(
        self,
        md: Markdown,
        *,
        config: MacrosConfig,
    ) -> None:
        self.md: Markdown = md
        self.config = config

    def run(self, lines: list[str]) -> list[str]:
        """Render body as Jinja2 template with built context."""
        # Fetch rendering context from our context preprocessor
        context = ContextPreprocessor.from_markdown(self.md)
        page = context.page if context else None
        project_config = context.config if context else {}
        project_root = Path(project_config.get("root_dir", ".")).resolve()

        # Don't render if not enabled by default and no page-level override
        if (
            not self.config.render_by_default
            and (not page or (page and not page.meta.get("render_macros")))
        ) or (page and page.meta.get("render_macros") is False):
            return lines

        text = "\n".join(lines)
        variables = {}
        macros = {}
        filters: dict[str, Callable] = {
            "pretty": _pretty,
            "fix_url": _fix_url,
        }

        # Merge extra into variables
        if extra := project_config.get("extra"):
            variables["extra"] = extra
            variables.update(extra)

        # Load YAML from configuration
        _merge_include_yaml(
            self.config.include_yaml,
            project_root,
            variables,
        )

        # Load YAML data from page metadata
        if page:
            _merge_include_yaml(
                page.meta.get("include_yaml", []),
                project_root,
                variables,
            )

        # Load module.
        # Relative path (without extension) or importable module name
        if self.config.module_name:
            mod_vars, mod_macros, mod_filters = _load_module(
                self.config.module_name,
                project_root,
            )
            variables.update(mod_vars)
            macros.update(mod_macros)
            filters.update(mod_filters)

        # Load pluglets (preinstalled modules)
        # Importable module names only
        for plug in self.config.modules:
            plug_vars, plug_macros, plug_filters = _load_module(plug)
            variables.update(plug_vars)
            macros.update(plug_macros)
            filters.update(plug_filters)

        # Merge page metadata
        if page:
            variables.update(page.meta)

        # Build Jinja2 environment
        env_kw: dict[str, Any] = {
            "block_start_string": self.config.j2_block_start_string,
            "block_end_string": self.config.j2_block_end_string,
            "variable_start_string": self.config.j2_variable_start_string,
            "variable_end_string": self.config.j2_variable_end_string,
        }
        if self.config.j2_comment_start_string is not None:
            env_kw["comment_start_string"] = self.config.j2_comment_start_string
        if self.config.j2_comment_end_string is not None:
            env_kw["comment_end_string"] = self.config.j2_comment_end_string
        if self.config.on_undefined == "strict":
            env_kw["undefined"] = jinja2.StrictUndefined
        if self.config.j2_extensions:
            env_kw["extensions"] = self.config.j2_extensions

        if (
            self.config.include_dir
            and (
                include_dir_path := project_root / self.config.include_dir
            ).exists()
        ):
            env_kw["loader"] = jinja2.FileSystemLoader(include_dir_path)

        env = jinja2.Environment(**env_kw)  # noqa: S701

        # Store builtin Jinja filters before adding our own
        builtin_filters = env.filters.copy()

        # Add user macros and filters to the environment
        env.filters.update(filters)
        env.globals.update(macros)

        # Add our own macros and variables to the environment
        env_globals: dict[str, Any] = {
            "config": project_config,
            "context": _context_closure(variables),
            "environment": _get_env_info(),
            "files": [],
            "filters": filters,
            "filters_builtin": builtin_filters,
            "git": _get_git_info(),
            "macros": macros,
            "navigation": [],
            "now": _now,
            "plugin": asdict(self.config),
        }
        if page:
            env_globals["page"] = page
        env.globals.update(env_globals)
        # This copies the environment filters and globals
        # into a new environment so this call must be last
        env.globals["macros_info"] = _macros_info_closure(env)  # ty:ignore[invalid-assignment]

        # Make global variables accessible to `context()` macro
        variables.update(env.globals)

        # Render title if it contains Jinja2 syntax
        title = variables.get("title")
        if isinstance(title, str) and (
            self.config.j2_variable_start_string in title
            or self.config.j2_block_start_string in title
        ):
            try:
                title_template = env.from_string(title)
                new_title = title_template.render(**variables)
            except Exception:  # noqa: BLE001
                pass
            else:
                variables["title"] = new_title
                if page:
                    page.meta["title"] = new_title

        # Render body and return it
        try:
            template = env.from_string(text)
            rendered = template.render(**variables)
        except Exception:
            if self.config.on_error_fail:
                raise
            rendered = text
        return rendered.split("\n")


class MacrosExtension(Extension):
    """Jinja2 templating with variables, macros, and filters."""

    name = "zensical.extensions.macros"

    def __init__(self, **kwargs: Any) -> None:
        self._kwargs: dict[str, Any] = kwargs

    def extendMarkdown(self, md: Markdown) -> None:
        md.registerExtension(self)
        config = MacrosConfig(**self._kwargs)
        md.preprocessors.register(
            MacrosPreprocessor(md, config=config),
            MacrosPreprocessor.name,
            priority=20,
        )


# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def makeExtension(**kwargs: Any) -> MacrosExtension:
    """Register Markdown extension."""
    return MacrosExtension(**kwargs)


def _now() -> datetime:
    """Return current datetime (`datetime.now()`)."""
    return datetime.now()  # noqa: DTZ005


def _macros_info_closure(env: Environment) -> Callable[[], str]:
    new_env = jinja2.Environment()  # noqa: S701
    new_env.filters.update(env.filters)
    new_env.globals.update(env.globals)

    def macros_info() -> str:
        """Display info about the macros environment, for debugging purposes."""
        return new_env.from_string(_MACROS_INFO).render()

    return macros_info


def _fix_url(url: str) -> str:
    parsed = urlparse(url)
    if (not parsed.scheme) and parsed.path:
        return "../" + url
    return url


def _list_items(obj: Any) -> Iterable[tuple[str | int, Any]]:
    try:
        return sorted(obj.items())
    except AttributeError:
        return sorted(obj.__dict__.items())
    except TypeError:
        return enumerate(list(obj))


def _format_value(value: Any) -> str:
    if callable(value):
        if doc := value.__doc__:
            doc = doc.strip().split("\n", 1)[0]
        else:
            return ""
        try:
            param_names = ", ".join(inspect.signature(value).parameters)
        except ValueError:
            return doc
        else:
            return f"(*{param_names}*)<br>{doc}" if param_names else doc
    elif isinstance(value, dict):
        r_list = []
        for key, val in _list_items(value):
            if isinstance(val, (int, float, str, list, dict)) or val is None:
                r_list.append(f"**{key}** = {val!r}")
            else:
                r_list.append(f"**{key}** [*{type(val).__name__}*]")
        return ", ".join(r_list)
    else:
        return repr(value)


def _context_closure(
    variables: dict[str, Any],
) -> Callable[[Any], list[tuple[Any, Any, str]]]:
    def context(obj: Any = None) -> list[tuple[Any, Any, str]]:
        """Display macros context (single object or all context)."""
        if obj is None:
            obj = variables
        try:
            return [
                (var, type(value).__name__, _format_value(value))
                for var, value in _list_items(obj)
            ]
        except UndefinedError as e:
            return [("*Error!*", type(e).__name__, str(e))]
        except AttributeError:
            # Not an object or dictionary (int, str, etc.)
            return [(obj, type(obj).__name__, repr(obj))]

    return context


def _make_table(
    rows: list[tuple[str, str, str]],
    header: tuple[str, str, str],
) -> str:
    def _escape_cell(value: str) -> str:
        return value.replace("|", r"\|")

    header_line = " | ".join(_escape_cell(item) for item in header)
    separator_line = " | ".join(["---"] * len(header))
    body_lines = [
        " | ".join(_escape_cell(str(item)) for item in row) for row in rows
    ]
    return "\n".join([header_line, separator_line, *body_lines])


def _pretty(var_list: list[Any]) -> str:
    if not var_list:
        return ""
    rows = [
        (
            f"**{var}**",
            f"*{var_type}*",
            content.replace("\n", "<br>"),
        )
        for var, var_type, content in var_list
    ]
    try:
        return _make_table(rows, ("Variable", "Type", "Content"))
    except Exception as error:  # noqa: BLE001
        return f"#{type(error).__name__}: {error}\n{traceback.format_exc()}"


def _load_module(
    module_name: str, project_root: Path | None = None
) -> VariablesMacrosFiltersType:
    """Load a module by name (e.g. 'main')."""
    if project_root:
        for candidate in [
            project_root / f"{module_name}.py",
            project_root / module_name / "__init__.py",
        ]:
            if not candidate.exists() or not candidate.is_relative_to(
                project_root
            ):
                continue
            spec = importlib.util.spec_from_file_location(
                module_name, candidate
            )
            if spec and spec.loader:
                mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(mod)
                if hasattr(mod, "define_env"):
                    env = MacroEnv()
                    mod.define_env(env)
                    return env.variables, env.macros, env.filters
            break

    # Only try import for package-like names (no path separators or "..").
    if "/" in module_name or "\\" in module_name or ".." in module_name:
        return {}, {}, {}
    try:
        mod = importlib.import_module(module_name)
    except ImportError:
        pass
    else:
        if hasattr(mod, "define_env"):
            env = MacroEnv()
            mod.define_env(env)
            return env.variables, env.macros, env.filters
    return {}, {}, {}


def _load_one_yaml(
    path: str,
    project_root: Path,
) -> VariablesType | None:
    """Load a single YAML file. Path must be relative to project root."""
    p = (
        (project_root / path).resolve()
        if not Path(path).is_absolute()
        else Path(path).resolve()
    )
    if not p.exists() or not p.is_relative_to(project_root):
        return None
    try:
        with open(p, encoding="utf-8") as f:
            data = yaml.safe_load(f)
    except (OSError, ValueError):
        return None
    else:
        return data if isinstance(data, dict) else None


def _merge_include_yaml(
    include_yaml: list[str] | dict[str, str],
    project_root: Path,
    variables: VariablesType,
) -> None:
    """Merge external data into variables."""
    if not include_yaml:
        return
    if isinstance(include_yaml, dict):
        for key, path in include_yaml.items():
            if data := _load_one_yaml(path, project_root):
                variables[key] = data
    else:
        for path in include_yaml:
            if data := _load_one_yaml(path, project_root):
                variables.update(data)


@cache
def _get_git_info() -> dict[str, Any]:
    """Return Git metadata for the current repository."""
    commands: dict[str, tuple[str, ...]] = {
        "short_commit": ("git", "rev-parse", "--short", "HEAD"),
        "commit": ("git", "rev-parse", "HEAD"),
        "tag": ("git", "describe", "--tags"),
        # With --abbrev set to 0, git finds the nearest tag name without suffix
        "short_tag": ("git", "describe", "--tags", "--abbrev=0"),
        "author": ("git", "log", "-1", "--pretty=format:%an"),
        "author_email": ("git", "log", "-1", "--pretty=format:%ae"),
        "committer": ("git", "log", "-1", "--pretty=format:%cn"),
        "committer_email": (
            "git",
            "log",
            "-1",
            "--pretty=format:%ce",
        ),
        # %cI is strict ISO 8601 commit date
        "date_ISO": ("git", "log", "-1", "--pretty=format:%cI"),
        "message": ("git", "log", "-1", "--pretty=format:%B"),
        "raw": ("git", "log", "-1"),
        "root_dir": ("git", "rev-parse", "--show-toplevel"),
    }

    result: dict[str, Any] = {"status": False, "date": None}

    for field_name, git_command in commands.items():
        try:
            output = subprocess.check_output(  # noqa: S603
                git_command,
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        except FileNotFoundError as error:  # noqa: PERF203
            # Git executable not found, abort early.
            return {
                "status": False,
                "diagnosis": "Git command not found",
                "error": str(error),
                "date": None,
            }
        except subprocess.CalledProcessError as error:
            if error.returncode == 128:  # noqa: PLR2004
                # Usually means no git repo or no tags.
                result[field_name] = ""
            else:
                result[field_name] = (
                    f"# Cannot execute '{git_command}': {error}"
                )
        except Exception as error:  # noqa: BLE001
            result[field_name] = f"# Unexpected error '{git_command}': {error}"
        else:
            result[field_name] = output
            if field_name == "date_ISO":
                result["date"] = datetime.fromisoformat(
                    output.replace("Z", "+00:00")
                )
            result["status"] = True

    return result


@cache
def _get_env_info() -> dict[str, str]:
    sys_name = platform.system() or "<UNKNOWN>"
    sys_name = {"Darwin": "MacOs"}.get(sys_name, sys_name)
    return {
        "system": sys_name,
        "system_version": platform.release(),
        "python_version": platform.python_version(),
        "mkdocs_version": "1.6.1",
        "macros_plugin_version": "1.3.7",
        "jinja2_version": jinja2.__version__ if jinja2 else "0.0.0",
    }
