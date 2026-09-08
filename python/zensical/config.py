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

import functools
import hashlib
import importlib
import os
import pickle
from importlib.metadata import EntryPoint, entry_points
from importlib.util import find_spec
from pathlib import Path
from typing import IO, TYPE_CHECKING, Any, cast
from urllib.parse import urljoin, urlparse

import yaml
from click import ClickException
from deepmerge import always_merger
from tomli import load as toml_load
from yaml import Loader, YAMLError
from yaml.constructor import ConstructorError

from zensical.extensions.autorefs import AutorefsExtension
from zensical.extensions.emoji import to_svg, twemoji
from zensical.extensions.glightbox import GlightboxExtension
from zensical.extensions.macros import MacrosExtension
from zensical.extensions.mkdocstrings import MkdocstringsExtension
from zensical.extensions.table_reader import TABLE_READERS, TableReaderExtension

if TYPE_CHECKING:
    from collections.abc import Iterable, Iterator


# ----------------------------------------------------------------------------
# Globals
# ----------------------------------------------------------------------------


_CONFIG: dict[str, Any] | None = None
"""
Global configuration to pick up later for parsing Markdown.

Since MkDocs uses YAML as a configuration format, the configuration can contain
references to functions or other Python objects, for which we don't have any
representation in Rust. Thus, we just keep the configuration on the Python
side, and use it directly when needed. It's a hack but will do for now.
"""


# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------


DEFAULT_MARKDOWN_EXTENSIONS = {
    "abbr": {},
    "admonition": {},
    "attr_list": {},
    "def_list": {},
    "footnotes": {},
    "md_in_html": {},
    "toc": {"permalink": True},
    "pymdownx.arithmatex": {"generic": True},
    "pymdownx.betterem": {},
    "pymdownx.caret": {},
    "pymdownx.details": {},
    "pymdownx.emoji": {
        "emoji_generator": to_svg,
        "emoji_index": twemoji,
    },
    "pymdownx.highlight": {
        "anchor_linenums": True,
        "line_spans": "__span",
        "pygments_lang_class": True,
    },
    "pymdownx.inlinehilite": {},
    "pymdownx.keys": {},
    "pymdownx.magiclink": {},
    "pymdownx.mark": {},
    "pymdownx.smartsymbols": {},
    "pymdownx.superfences": {
        "custom_fences": [{"name": "mermaid", "class": "mermaid"}]
    },
    "pymdownx.tabbed": {
        "alternate_style": True,
        "combine_header_slug": True,
    },
    "pymdownx.tasklist": {"custom_checkbox": True},
    "pymdownx.tilde": {},
}


# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class ConfigurationError(ClickException):
    """Configuration resolution or validation failed."""


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def parse_config(path: str) -> dict:
    """Parse configuration file."""
    # Decide by extension; no need to convert to Path
    _, ext = os.path.splitext(path)
    if ext.lower() == ".toml":
        return parse_zensical_config(path)
    return parse_mkdocs_config(path)


def parse_zensical_config(path: str) -> dict:
    """Parse zensical.toml configuration file."""
    global _CONFIG  # noqa: PLW0603
    with open(path, "rb") as f:
        config = toml_load(f)
    if "project" in config:
        config = config["project"]

    # Apply defaults and return parsed configuration
    _CONFIG = _apply_defaults(config, path)
    return _CONFIG


def parse_mkdocs_config(path: str) -> dict:
    """Parse mkdocs.yml configuration file."""
    global _CONFIG  # noqa: PLW0603
    with open(path, encoding="utf-8") as f:
        config = _yaml_load(f)

    # Apply defaults and return parsed configuration
    _CONFIG = _apply_defaults(config, path)
    return _CONFIG


def get_config() -> dict:
    """Return configuration."""
    # We assume this function is only called after populating `_CONFIG`.
    return _CONFIG  # ty:ignore[invalid-return-type]


def _yaml_load(source: IO) -> dict[str, Any]:
    """Load configuration file, resolve environment variables and parent files.

    Note that INHERIT is only a bandaid that was introduced to allow for some
    degree of modularity, but with serious shortcomings. Zensical will use a
    different approach in the future, which will allow for composable and
    environment-specific configuration.
    """
    Loader.add_constructor("!ENV", _construct_env_tag)
    try:
        config = yaml.load(
            # Compatibility shim: we remap Material's extension namespace to
            # Zensical's, and the now deprecated materialx namespace as well
            source.read()
            .replace("material.extensions", "zensical.extensions")
            .replace("materialx", "zensical.extensions"),
            Loader=Loader,  # noqa: S506
        )
    except YAMLError as e:
        raise ConfigurationError(
            f"Encountered an error parsing the configuration file: {e}"
        ) from e
    if config is None:
        return {}

    # Try to resolve inherited configuration file
    if "INHERIT" in config and not isinstance(source, str):
        relpath = config.pop("INHERIT")
        abspath = os.path.normpath(
            os.path.join(os.path.dirname(source.name), relpath)
        )
        if not os.path.exists(abspath):
            raise ConfigurationError(
                f"Inherited config file '{relpath}' "
                f"doesn't exist at '{abspath}'."
            )
        with open(abspath, encoding="utf-8") as fd:
            parent = _yaml_load(fd)
        config = always_merger.merge(parent, config)

    # Return resulting configuration
    return config


def _construct_env_tag(
    loader: yaml.Loader,
    node: yaml.ScalarNode | yaml.SequenceNode | yaml.MappingNode,
) -> Any:
    """Assign value of ENV variable referenced at node.

    MkDocs supports the use of !ENV to reference environment variables in YAML
    configuration files. We won't likely support this in Zensical, but for now
    we need it to build MkDocs projects. Zensical will use a different approach
    to create environment-specific configuration in the future.

    Licensed under MIT
    Copyright (c) 2020 Waylan Limberg
    Taken and adapted from
        https://github.com/waylan/pyyaml-env-tag/blob/master/yaml_env_tag.py
    """
    default = None

    # Handle !ENV <name>
    if isinstance(node, yaml.nodes.ScalarNode):
        vars = [loader.construct_scalar(node)]

    # Handle !ENV [<name>, <fallback>]
    elif isinstance(node, yaml.nodes.SequenceNode):
        child_nodes = node.value
        if len(child_nodes) > 1:
            default = loader.construct_object(child_nodes[-1])
            child_nodes = child_nodes[:-1]
        # Env Vars are resolved as string values, ignoring (implicit) types.
        vars = [loader.construct_scalar(child) for child in child_nodes]
    else:
        raise ConstructorError(
            context=f"expected a scalar or sequence node, but found {node.id}",
            context_mark=node.start_mark,
        )

    # Resolve environment variable
    for var in vars:
        if var in os.environ:
            value = os.environ[var]
            # Resolve value to Python type using YAML's implicit resolvers
            tag = loader.resolve(yaml.nodes.ScalarNode, value, (True, False))
            return loader.construct_object(yaml.nodes.ScalarNode(tag, value))

    # Otherwise return default
    return default


# ----------------------------------------------------------------------------


@functools.cache
def get_themes() -> dict[str, EntryPoint]:
    """Return available themes.

    This function adds support for using MkDocs themes with Zensical, including
    derived themes. There's currently no plan to add `zensical.*` entrypoints,
    since the theming system will undergo a major revision with the addition
    of the component system. This will make theming simpler and modular.
    """
    themes: dict[str, EntryPoint] = {}
    for entry_point in entry_points(group="mkdocs.themes"):
        themes[entry_point.name] = entry_point

    # Return themes
    return themes


def get_builtin_theme_dir() -> str:
    """Return the built-in theme directory."""
    path = os.path.dirname(os.path.abspath(__file__))
    return os.path.join(path, "templates")


def get_theme_dir(name: str) -> str:
    """Return the theme directory."""
    # Zensical's default theme is the replacement for the `material` theme, so
    # make sure to use the built-in theme when `material` is specified.
    # We also reserve the `zensical` theme name to the same effect.
    if name in ("material", "zensical"):
        return get_builtin_theme_dir()

    # Otherwise, check for installed themes - note that because of minijinja,
    # theme might need to be adjusted. This functionality is primarily intended
    # for extending the `material` theme, as some users did, and allow to move
    # overrides into an installable python package.
    themes = get_themes()
    if name not in themes:
        raise ConfigurationError(
            f"Theme '{name}' is not installed. "
            f"Available themes are: {', '.join(themes.keys())}"
        )
    return os.path.dirname(os.path.abspath(themes[name].load().__file__))


def get_custom_theme_dir(path: str, config_path: str) -> str:
    """Return the custom theme directory."""
    theme_dir = os.path.normpath(
        os.path.join(os.path.dirname(config_path), path)
    )

    # Validate that custom theme directory exists
    if not os.path.isdir(theme_dir):
        raise ConfigurationError(
            f"Custom theme directory does not exist: {theme_dir}"
        )
    return theme_dir


def _yaml_load_theme_config(source: IO) -> dict[str, Any]:
    Loader.add_constructor("!ENV", _construct_env_tag)
    try:
        config = yaml.load(source, Loader=Loader)  # noqa: S506
    except YAMLError as e:
        raise ConfigurationError(
            f"Encountered an error parsing the theme configuration file: {e}"
        ) from e
    if config is None:
        return {}
    return config


def _load_theme_config(theme_dir: str) -> tuple[dict[str, Any], list[str]]:
    # Keep track of the theme directories,
    # so that we can pass them up to Minijinja.
    theme_dirs = [theme_dir]

    if (theme_config_file := Path(theme_dir, "mkdocs_theme.yml")).is_file():
        # We found a theme configuration file (mkdocs_theme.yml).
        with theme_config_file.open(encoding="utf-8") as file:
            theme_config = _yaml_load_theme_config(file)
            if extends := theme_config.pop("extends", None):
                # This theme extends another theme,
                # so we load this parent theme's configuration (recursively)
                # and merge the current theme's configuration into it.
                parent_theme_dir = get_theme_dir(extends)
                parent_config, parent_dirs = _load_theme_config(
                    parent_theme_dir
                )
                parent_config.update(theme_config)
                theme_dirs.extend(parent_dirs)
                return parent_config, theme_dirs
            # Otherwise we just return the current theme configuration.
            return theme_config, theme_dirs

    # No theme configuration.
    return {}, theme_dirs


# ----------------------------------------------------------------------------


def _apply_defaults(config: dict, path: str) -> dict:
    """Apply default settings in configuration.

    Note that this is loosely based on the defaults that MkDocs sets in its own
    configuration system, which we won't port for compatibility right now, as
    well as the defaults that are set in Material for MkDocs for theme- and
    extra-level settings.

    We must set all properties, as well as nested properties to `None`, or PyO3
    will refuse to convert them, as the key must definitely exist.
    """
    config["root_dir"] = os.path.dirname(path)
    project_root = Path(config["root_dir"]).resolve()

    if "site_name" not in config:
        raise ConfigurationError("Missing required setting: site_name")

    # Set site directory
    set_default(config, "site_dir", "site", str)
    if config["site_dir"] == "":
        raise ConfigurationError("site_dir must not be empty")
    site_dir = Path(config["site_dir"])
    if site_dir.is_absolute():
        site_dir = site_dir.resolve()
    else:
        site_dir = project_root.joinpath(site_dir).resolve()
    if not site_dir.is_relative_to(project_root):
        raise ConfigurationError("site_dir must be within project root")

    # Set docs directory
    set_default(config, "docs_dir", "docs", str)
    if config["docs_dir"] == "":
        raise ConfigurationError("docs_dir must not be empty")
    docs_dir = Path(config["docs_dir"])
    if docs_dir.is_absolute():
        docs_dir = docs_dir.resolve()
    else:
        docs_dir = project_root.joinpath(docs_dir).resolve()
    if not docs_dir.is_relative_to(project_root):
        raise ConfigurationError("docs_dir must be within project root")

    # Validate that site_dir is not the same as docs_dir
    if site_dir == docs_dir:
        raise ConfigurationError("site_dir and docs_dir must be different")

    # Validate that docs directory exists
    if not docs_dir.is_dir():
        raise ConfigurationError(f"Docs directory does not exist: {docs_dir}")

    # Set defaults for core settings
    set_default(config, "site_url", None, str)
    set_default(config, "site_description", None, str)
    set_default(config, "site_author", None, str)
    set_default(config, "use_directory_urls", True, bool)
    set_default(config, "strict", False, bool)
    set_default(config, "dev_addr", "localhost:8000", str)
    set_default(config, "copyright", None, str)
    set_default(config, "watch", [], list)

    # Validate watch setting
    if not all(isinstance(path, str) for path in config["watch"]):
        raise ConfigurationError("'watch' entries must be strings.")

    # Set defaults for versioning with mike
    set_default(config, "remote_branch", "gh-pages", str)
    set_default(config, "remote_name", "origin", str)

    # Set defaults for repository settings
    set_default(config, "repo_url", None, str)
    set_default(config, "repo_name", None, str)
    set_default(config, "edit_uri_template", None, str)
    set_default(config, "edit_uri", None, str)

    # Set defaults for repository name settings
    repo_names = {
        "github.com": "GitHub",
        "gitlab.com": "Gitlab",
        "bitbucket.org": "Bitbucket",
    }
    rel_docs_dir = docs_dir.relative_to(project_root)
    edit_uris = {
        "github.com": f"edit/master/{rel_docs_dir}",
        "gitlab.com": f"edit/master/{rel_docs_dir}",
        "bitbucket.org": f"src/default/{rel_docs_dir}",
    }
    repo_url = config.get("repo_url")
    if repo_url:
        host = urlparse(repo_url).hostname or ""
        if not config.get("repo_name"):
            if host in repo_names:
                set_default(config, "repo_name", repo_names[host], str)
            elif host:
                config["repo_name"] = host.split(".")[0].title()
        if host in edit_uris:
            set_default(config, "edit_uri", edit_uris[host], str)

    # Remove trailing slash from edit_uri if present
    edit_uri = config.get("edit_uri")
    if isinstance(edit_uri, str) and edit_uri.endswith("/"):
        config["edit_uri"] = edit_uri.rstrip("/")

    # Set defaults for theme font settings
    if "theme" in config and isinstance(config["theme"], str):
        config["theme"] = {"name": config["theme"]}
    elif "theme" not in config:
        config["theme"] = {}

    # Set defaults for custom theme directory
    set_default(config["theme"], "custom_dir", None, str)

    # Load theme configuration
    if config["theme"].get("custom_dir"):
        theme_dir = get_custom_theme_dir(config["theme"]["custom_dir"], path)
        theme_config, theme_dirs = _load_theme_config(theme_dir)
        if (
            len(theme_dirs) == 1
            and (theme_name := config["theme"].get("name", "material"))
            is not None
        ):
            # Custom theme doesn't extend another theme,
            # but user specified a theme name in configuration,
            # so we use it as base theme.
            theme_dir = get_theme_dir(theme_name)
            base_theme_config, base_theme_dirs = _load_theme_config(theme_dir)
            theme_config = {**base_theme_config, **theme_config}
            theme_dirs = theme_dirs + base_theme_dirs
    else:
        # No custom theme directory
        theme_dir = get_theme_dir(config["theme"].get("name") or "material")
        theme_config, theme_dirs = _load_theme_config(theme_dir)

    # Store theme directories for Minijinja and merge theme configuration
    config["theme_dirs"] = theme_dirs
    config["theme"] = {**theme_config, **config["theme"]}

    theme = config["theme"]

    # Set defaults for theme name
    # (we do this after loading the theme configuration
    # so that explicitly setting the name to null
    # tells the system not to extend the Material theme)
    set_default(theme, "name", None, str)

    # Set variant and fonts for variant
    set_default(theme, "variant", "modern", str)
    if theme.get("variant") == "modern":
        font = {"text": "Inter", "code": "JetBrains Mono"}
    else:
        font = {"text": "Roboto", "code": "Roboto Mono"}

    # Ensure presence of static templates
    theme["static_templates"] = ["404.html", "sitemap.xml"]

    # Set defaults for theme settings
    set_default(theme, "language", "en", str)
    set_default(theme, "direction", None, str)
    set_default(theme, "features", [], list)
    set_default(theme, "favicon", "assets/images/favicon.png", str)
    set_default(theme, "logo", None, str)

    # Set defaults for theme font settings
    theme.setdefault("font", {})
    if isinstance(theme["font"], dict):
        set_default(theme["font"], "text", font["text"], str)
        set_default(theme["font"], "code", font["code"], str)

    # Set defaults for theme icons
    icon = set_default(theme, "icon", {}, dict)
    set_default(icon, "repo", None, str)
    set_default(icon, "annotation", None, str)
    set_default(icon, "tag", {}, dict)
    if theme.get("variant") == "modern":
        set_default(icon, "logo", "lucide/book-open", str)
        set_default(icon, "edit", "lucide/file-pen", str)
        set_default(icon, "view", "lucide/file-code-2", str)
        set_default(icon, "top", "lucide/circle-arrow-up", str)
        set_default(icon, "share", "lucide/share-2", str)
        set_default(icon, "menu", "lucide/menu", str)
        set_default(icon, "alternate", "lucide/languages", str)
        set_default(icon, "search", "lucide/search", str)
        set_default(icon, "close", "lucide/x", str)
        set_default(icon, "previous", "lucide/arrow-left", str)
        set_default(icon, "next", "lucide/arrow-right", str)
    else:
        set_default(icon, "logo", None, str)
        set_default(icon, "edit", None, str)
        set_default(icon, "view", None, str)
        set_default(icon, "top", None, str)
        set_default(icon, "share", None, str)
        set_default(icon, "menu", None, str)
        set_default(icon, "alternate", None, str)
        set_default(icon, "search", None, str)
        set_default(icon, "close", None, str)
        set_default(icon, "previous", None, str)
        set_default(icon, "next", None, str)

    # Set defaults for theme admonition icons
    admonition = set_default(icon, "admonition", {}, dict)
    if isinstance(admonition, dict):
        icon["admonition"] = {
            str(key): str(value)
            for key, value in admonition.items()
            if value is not None
        }

    # Set defaults for theme palette settings and normalize to list
    palette = theme.setdefault("palette", [])
    if isinstance(palette, dict):
        palette = [palette]
        theme["palette"] = palette

    # Set defaults for each palette entry
    for entry in palette:
        set_default(entry, "media", None, str)
        set_default(entry, "scheme", None, str)
        set_default(entry, "primary", None, str)
        set_default(entry, "accent", None, str)
        set_default(entry, "toggle", None, dict)

        # Set defaults for palette toggle
        toggle = entry.get("toggle")
        if toggle:
            set_default(toggle, "icon", None, str)
            set_default(toggle, "name", None, str)

    # Set defaults for extra settings
    if "extra" in config and not isinstance(config["extra"], dict):
        raise ConfigurationError(
            "The 'extra' setting must be a mapping/dictionary."
        )
    extra = set_default(config, "extra", {}, dict)

    if "polyfills" in extra and not isinstance(extra["polyfills"], list):
        raise ConfigurationError(
            "The 'extra.polyfills' setting must be a list."
        )
    set_default(extra, "polyfills", [], list)

    # Ensure all non-existent values are all empty strings (for now)
    config["extra"] = _convert_extra(extra)

    # Set defaults for extra files
    set_default(config, "extra_css", [], list)
    set_default(config, "extra_templates", [], list)

    # Generate navigation if not defined, and convert
    config["nav"] = _convert_nav(config.setdefault("nav", []))
    config["extra_javascript"] = _convert_extra_javascript(
        config.setdefault("extra_javascript", [])
    )

    # Initialize defaults for validation
    validation = {
        "unresolved_references": False,
        "unresolved_footnotes": False,
        "unused_definitions": False,
        "unused_footnotes": False,
        "shadowed_definitions": False,
        "shadowed_footnotes": False,
        "invalid_links": True,
        "invalid_link_anchors": True,
    }

    # Map MkDocs validation configuration to ours - note that we only support
    # validation of links right now, as navigation will change significantly
    if "validation" in config:
        if isinstance(config["validation"], bool):
            config["validation"] = {
                "invalid_links": config["validation"],
                "invalid_link_anchors": config["validation"],
            }

        # Hoist links configuration to the top level, if present
        input = config["validation"]
        if "links" in input:
            input.update(input.pop("links"))

        # We only support a subset of MkDocs' validation settings, so we ignore
        # the ones we don't support. We also map info to warn for simplicity.
        if "not_found" in input:
            validation["invalid_links"] = input["not_found"] != "ignore"
        if "anchors" in input:
            validation["invalid_link_anchors"] = input["anchors"] != "ignore"

        # Our own keys override the ones we map from MkDocs, so we apply them
        # after mapping the MkDocs keys
        for key in validation:
            if key in input:
                validation[key] = bool(input[key])

    # Set validation
    config["validation"] = validation

    # MkDocs will also set fenced_code, which is incompatible with SuperFences,
    # the extension that Material for MkDocs generally recommends. Note that we
    # decided to set defaults that make it easy to get started with sensible
    # Markdown support, but users can override this as needed.
    markdown_extensions, mdx_configs = _convert_markdown_extensions(
        config.get("markdown_extensions", DEFAULT_MARKDOWN_EXTENSIONS)
    )
    config["markdown_extensions"] = markdown_extensions
    config["mdx_configs"] = mdx_configs

    # Now, since YAML supports using Python tags to resolve functions, we need
    # to support the same for when we load TOML. This is a bandaid, and we will
    # find a better solution, once we work on configuration management, but for
    # now this should be sufficient.
    _resolve_pymdownx_emoji(config)
    _resolve_pymdownx_superfences(config)
    _resolve_pymdownx_tabbed(config)
    _resolve_pymdownx_blocks_tab(config)
    _resolve_toc(config)

    # Ensure the table of contents title is initialized, as it's used inside
    # the template, and the table of contents extension is always defined
    config["mdx_configs"]["toc"].setdefault("title", None)
    config["mdx_configs_hash"] = _hash(mdx_configs)

    # Convert plugins configuration
    config["plugins"] = _convert_plugins(config.get("plugins", []), config)

    # Map plugins configuration to Markdown extensions
    _shim_autorefs(config)
    _shim_markdown_exec(config)
    _shim_mkdocstrings(config)
    _shim_glightbox(config)
    _shim_macros(config)
    _shim_table_reader(config)

    # List files along with their hashes, so we can rebuild when they change
    watched_files = (
        _list_sources(config, path)  # mkdocstrings
        | _list_snippet_files(config, path)  # pymdownx.snippets
        | _list_macros_files(config, path)  # macros
        | _list_watch_files(config, path)  # watch
    )

    # We watch theme directories by default on the Rust side,
    # so we need to  prevent duplicates from here, in case
    # users add theme directories to the watch option
    theme_files = _list_templates(config)
    watched_files -= set(theme_files)
    config["watched_files"] = sorted(watched_files)

    # Hash all templates, so we rebuild if something changes
    config["template_hash"] = _hash(theme_files)

    # Hash the entire plugins configuration.
    # This is a special case for plugins because we currently only source
    # the plugin configuration that we support in Rust,
    # which means config on other plugins doesn't contribute to the hash,
    # in turn not triggering full rebuilds.
    config["plugins_hash"] = _hash(config["plugins"])

    return config


def set_default(
    entry: dict, key: str, default: Any, data_type: type | None = None
) -> Any:
    """Set a key to a default value if it isn't set.

    Optionally cast it to the specified data type.
    """
    if key in entry and entry[key] is None:
        del entry[key]

    # Set the default value if the key is not present
    entry.setdefault(key, default)

    # Optionally cast the value to the specified data type
    if data_type is not None and entry[key] is not None:
        try:
            entry[key] = data_type(entry[key])
        except (ValueError, TypeError) as e:
            raise ValueError(
                f"Failed to cast key '{key}' to {data_type}: {e}"
            ) from e

    # Return the resulting value
    return entry[key]


def _hash(data: Any) -> int:
    """Compute a hash for the given data."""
    hash = hashlib.sha1(pickle.dumps(data))  # noqa: S324
    return int(hash.hexdigest(), 16) % (2**64)


# ----------------------------------------------------------------------------


def _resolve(symbol: str) -> Any:
    """Resolve a symbol to its corresponding Python object."""
    module_path, func_name = symbol.rsplit(".", 1)
    module = importlib.import_module(module_path)
    return getattr(module, func_name)


def _resolve_pymdownx_emoji(config: dict[str, Any]) -> None:
    # Emoji extension: resolve emoji generator and index functions
    emoji = config["mdx_configs"].get("pymdownx.emoji", {})
    if isinstance(emoji.get("emoji_generator"), str):
        emoji["emoji_generator"] = _resolve(emoji.get("emoji_generator"))
    if isinstance(emoji.get("emoji_index"), str):
        emoji["emoji_index"] = _resolve(emoji.get("emoji_index"))


def _resolve_pymdownx_tabbed(config: dict[str, Any]) -> None:
    # Tabbed extension: resolve slugification function
    tabbed = config["mdx_configs"].get("pymdownx.tabbed", {})
    if isinstance(tabbed.get("slugify"), dict):
        object = tabbed["slugify"].get("object", "pymdownx.slugs.slugify")
        tabbed["slugify"] = _resolve(object)(
            **tabbed["slugify"].get("kwds", {})
        )


def _resolve_pymdownx_blocks_tab(config: dict[str, Any]) -> None:
    # Blocks-tab extension: resolve slugification function
    blocks_tab = config["mdx_configs"].get("pymdownx.blocks.tab", {})
    if isinstance(blocks_tab.get("slugify"), dict):
        object = blocks_tab["slugify"].get("object", "pymdownx.slugs.slugify")
        blocks_tab["slugify"] = _resolve(object)(
            **blocks_tab["slugify"].get("kwds", {})
        )


def _resolve_pymdownx_superfences(config: dict[str, Any]) -> None:
    # Superfences extension: resolve format and validator functions
    superfences = config["mdx_configs"].get("pymdownx.superfences", {})
    for fence in superfences.get("custom_fences", []):
        if isinstance(fence.get("format"), str):
            fence["format"] = _resolve(fence.get("format"))
        elif isinstance(fence.get("format"), dict):
            object = fence["format"].get(
                "object", "pymdownx.superfences.fence_code_format"
            )
            fence["format"] = _resolve(object)(
                **fence["format"].get("kwds", {})
            )
        if isinstance(fence.get("validator"), str):
            fence["validator"] = _resolve(fence.get("validator"))
        elif isinstance(fence.get("validator"), dict):
            object = fence["validator"].get("object")
            callable_object = (
                _resolve(object) if object else lambda *args, **kwargs: True
            )
            fence["validator"] = callable_object(
                **fence["validator"].get("kwds", {})
            )


def _resolve_toc(config: dict[str, Any]) -> None:
    # Table of contents extension: resolve slugification function
    toc = config["mdx_configs"]["toc"]
    if isinstance(toc.get("slugify"), dict):
        object = toc["slugify"].get("object", "pymdownx.slugs.slugify")
        toc["slugify"] = _resolve(object)(**toc["slugify"].get("kwds", {}))


# ----------------------------------------------------------------------------


def _shim_autorefs(config: dict[str, Any]) -> None:
    # The Markdown extension is already enabled
    if AutorefsExtension.name in config["markdown_extensions"]:
        return
    # Map autorefs plugin configuration to the extension configuration
    if "autorefs" in config["plugins"]:
        plugin = config["plugins"]["autorefs"]["config"]
        if plugin.get("enabled", True):
            config["markdown_extensions"].append(AutorefsExtension.name)
    elif "mkdocstrings" in config["plugins"]:
        # mkdocstrings enables autorefs itself
        plugin = config["plugins"]["mkdocstrings"]["config"]
        if plugin.get("enabled", True):
            config["markdown_extensions"].append(AutorefsExtension.name)
    elif "zensical.extensions.mkdocstrings" in config["markdown_extensions"]:
        # same when mkdocstrings is enabled as a Markdown extension
        config["markdown_extensions"].append(AutorefsExtension.name)


def _shim_markdown_exec(config: dict[str, Any]) -> None:
    # Map markdown-exec plugin configuration to superfences configuration
    if "markdown-exec" in config["plugins"]:
        try:
            import markdown_exec  # noqa: PLC0415  # ty:ignore[unresolved-import]
        except ImportError as error:
            raise ConfigurationError(
                "markdown-exec plugin is enabled, but markdown-exec is not "
                "installed. Please install markdown-exec or disable the plugin."
            ) from error
        markdown_exec_config = config["plugins"]["markdown-exec"]["config"]
        enabled = markdown_exec_config.pop("enabled", True)
        languages = markdown_exec_config.get("languages", None)
        if enabled and (languages or languages is None):
            # Check ANSI colors requirement and pygments-ansi-color presence.
            trueish = ("auto", "required", True)
            ansi = markdown_exec_config.get("ansi", False) in trueish
            if ansi and not find_spec("pygments_ansi_color"):
                raise ConfigurationError(
                    "ANSI colors are required, but pygments-ansi-color is not "
                    "installed. Please install pygments-ansi-color or turn off "
                    "ANSI requirement in markdown-exec configuration."
                )
            # Update superfences configuration.
            if "pymdownx.superfences" not in config["markdown_extensions"]:
                config["markdown_extensions"].append("pymdownx.superfences")
            if "pymdownx.superfences" not in config["mdx_configs"]:
                config["mdx_configs"]["pymdownx.superfences"] = {}
            superfences = config["mdx_configs"]["pymdownx.superfences"]
            if "custom_fences" not in superfences:
                superfences["custom_fences"] = []
            superfences["custom_fences"].extend(
                _get_markdown_exec_superfences(languages)
            )
        # Save configuration in markdown-exec.
        # We must pass the original, mutable references,
        # since other shims and our `render` function might modify them.
        markdown_exec.markdown_config.save(
            config["markdown_extensions"],
            config["mdx_configs"],
        )


def _shim_mkdocstrings(config: dict[str, Any]) -> None:
    # The Markdown extension is already enabled
    if MkdocstringsExtension.name in config["markdown_extensions"]:
        return
    # Map mkdocstrings plugin configuration to the extension configuration
    if "mkdocstrings" in config["plugins"]:
        plugin = config["plugins"]["mkdocstrings"]["config"]
        if plugin.get("enabled", True):
            if not find_spec("mkdocstrings"):
                raise ConfigurationError(
                    "mkdocstrings plugin is enabled, but mkdocstrings is not "
                    "installed. Please install mkdocstrings or disable the "
                    "plugin."
                )
            config["markdown_extensions"].append(MkdocstringsExtension.name)
            config["mdx_configs"][MkdocstringsExtension.name] = plugin


def _shim_glightbox(config: dict[str, Any]) -> None:
    # The Markdown extension is already enabled
    if GlightboxExtension.name in config["markdown_extensions"]:
        return
    # Map glightbox plugin configuration to the extension configuration
    if "glightbox" in config["plugins"]:
        plugin = config["plugins"]["glightbox"]["config"]
        if plugin.get("enabled", True):
            config["markdown_extensions"].append(GlightboxExtension.name)
            config["mdx_configs"][GlightboxExtension.name] = plugin


def _shim_macros(config: dict[str, Any]) -> None:
    # The Markdown extension is already enabled
    if MacrosExtension.name in config["markdown_extensions"]:
        return
    # Map macros plugin configuration to the extension configuration
    if "macros" in config["plugins"]:
        plugin = config["plugins"]["macros"]["config"]
        if plugin.get("enabled", True):
            config["markdown_extensions"].append(MacrosExtension.name)
            config["mdx_configs"][MacrosExtension.name] = plugin


def _shim_table_reader(config: dict[str, Any]) -> None:
    """Map table-reader configuration to its compatibility extension."""
    if "table-reader" not in config["plugins"]:
        return

    plugin = config["plugins"]["table-reader"]["config"]
    if not plugin.get("enabled", True):
        return

    # With macros enabled, the shared readers are added directly to its Jinja
    # environment, matching the integration behavior of the MkDocs plugins.
    macros_config = config["mdx_configs"].get(MacrosExtension.name, {})
    macros_enabled = MacrosExtension.name in config[
        "markdown_extensions"
    ] and macros_config.get("enabled", True)
    if (
        macros_enabled
        or TableReaderExtension.name in config["markdown_extensions"]
    ):
        return

    config["markdown_extensions"].append(TableReaderExtension.name)
    config["mdx_configs"][TableReaderExtension.name] = plugin


# ----------------------------------------------------------------------------


def _ignore_directory(dirpath: str) -> bool:
    """Determine whether to ignore a folder based on its name."""
    return dirpath == "__pycache__" or dirpath.startswith(".venv")


def _get_markdown_exec_superfences(
    languages: list[str] | None = None,
) -> list[dict[str, Any]]:
    """Get superfences configuration for markdown-exec."""
    import markdown_exec  # noqa: PLC0415  # ty:ignore[unresolved-import]

    return [
        {
            "name": language,
            "class": language,
            "validator": markdown_exec.validator,
            "format": markdown_exec.formatter,
        }
        for language in languages or markdown_exec.formatters.keys()
    ]


def _list_py_modules(path: Path) -> Iterator[Path]:
    """List Python modules in a directory, recursively."""
    for root, dirs, files in os.walk(path, topdown=True, followlinks=True):
        dirs[:] = [dir for dir in dirs if not _ignore_directory(dir)]
        for relfile in files:
            if os.path.splitext(relfile)[1] in {
                ".py",
                ".pyc",
                ".pyo",
                ".pyd",
                ".pyi",
                ".so",
            }:
                yield Path(root, relfile)


def _list_sources(config: dict, config_file: str) -> set[tuple[str, int]]:
    """List all absolute links to source files for mkdocstrings."""
    python_paths = (
        config["plugins"]
        .get("mkdocstrings", {})
        .get("config", {})
        .get("handlers", {})
        .get("python", {})
        .get("paths", ())
    )
    files_with_hash = set()
    root = Path(config_file).parent.resolve()
    for python_path in python_paths:
        path = root.joinpath(python_path).resolve()
        if path.is_dir() and path.is_relative_to(root) and path != root:
            for py_module in _list_py_modules(path):
                files_with_hash.add(
                    (str(py_module), int(os.path.getmtime(py_module)))
                )
    return files_with_hash


def _list_snippet_files(config: dict, config_file: str) -> set[tuple[str, int]]:
    """List files referenced in pymdownx.snippets auto_append configuration."""
    snippets_config = config["mdx_configs"].get("pymdownx.snippets", {})
    auto_append = snippets_config.get("auto_append", [])
    base_paths = snippets_config.get("base_path", ["."])

    root = Path(config_file).parent.resolve()
    files_with_mtime = set()
    for file_name in auto_append:
        for base_path in base_paths:
            candidate = root.joinpath(base_path, file_name).resolve()
            if candidate.is_file():
                mtime = int(os.path.getmtime(candidate))
                files_with_mtime.add((str(candidate), mtime))
                break

    return files_with_mtime


def _list_macros_files(config: dict, config_file: str) -> set[tuple[str, int]]:
    """List files referenced in macros plugin/extension."""
    root = Path(config_file).parent.resolve()
    macros_config = config["mdx_configs"].get(MacrosExtension.name, {})
    macros_files = []
    files_with_mtime = set()

    module = macros_config.get("module", "main")
    if (module_path := root.joinpath(module + ".py").resolve()).is_file():
        macros_files.append(module_path)

    pluglets = macros_config.get("modules", [])
    for pluglet in pluglets:
        try:
            pluglet_module = importlib.import_module(pluglet)
        except ImportError:  # noqa: PERF203
            continue
        else:
            macros_files.append(pluglet_module.__file__)

    include_yaml: list[str] | dict[str, str] = macros_config.get(
        "include_yaml", []
    )
    if isinstance(include_yaml, dict):
        include_yaml = list(include_yaml.values())
    for yaml_file in include_yaml:
        candidate = root.joinpath(yaml_file).resolve()
        if candidate.is_file():
            macros_files.append(candidate)

    for file_path in macros_files:
        mtime = int(os.path.getmtime(file_path))
        files_with_mtime.add((str(file_path), mtime))

    include_dir = macros_config.get("include_dir", None)
    if include_dir:
        candidate_dir = root.joinpath(include_dir).resolve()
        if candidate_dir.is_dir():
            for root, _, files in os.walk(candidate_dir):
                for file in files:
                    file_path = os.path.join(root, file)
                    mtime = int(os.path.getmtime(file_path))
                    files_with_mtime.add((file_path, mtime))

    return files_with_mtime


def _list_watch_files(config: dict, config_file: str) -> set[tuple[str, int]]:
    """List files for user-defined watch paths."""
    root = Path(config_file).parent.resolve()
    files_with_mtime: set[tuple[str, int]] = set()
    for watch_path in config.get("watch", []):
        path = root.joinpath(watch_path).resolve()
        if path.is_file():
            mtime = int(os.path.getmtime(path))
            files_with_mtime.add((str(path), mtime))
        elif path.is_dir():
            for dirpath, _, files in os.walk(path):
                for file in files:
                    file_path = os.path.join(dirpath, file)
                    mtime = int(os.path.getmtime(file_path))
                    files_with_mtime.add((file_path, mtime))
    return files_with_mtime


def _list_templates(config: dict) -> list[tuple[str, int]]:
    """List all template files in the theme directories."""
    # Collect file paths and their mtimes
    files_with_mtime = []
    for directory in config["theme_dirs"]:
        for root, _, files in os.walk(directory):
            if ".icons" in root:
                continue
            for file in files:
                file_path = os.path.join(root, file)
                mtime = int(os.path.getmtime(file_path))
                files_with_mtime.append((file_path, mtime))

    # Sort by file path for deterministic order
    return sorted(files_with_mtime)


# -----------------------------------------------------------------------------
# Note: we do a lot of validation on configuration settings in the following
# helpers (presence, shape, type, value). This is transitional: configuration
# parsing and validation will move to Rust at some point. Doing this in Python
# is an easy win until we start working on configuration in Rust.


def _is_index(path: str) -> bool:
    """Returns, whether the given path points to a section index."""
    return os.path.basename(path) in ("index.md", "README.md")


def _convert_nav(nav: list) -> list:
    """Convert MkDocs navigation."""
    return [_convert_nav_item(entry) for entry in nav]


def _convert_nav_item(item: str | dict | list) -> dict | list:
    """Convert MkDocs shorthand navigation structure into something manageable.

    We need to annotate each item with a title, URL, icon, and children.
    """
    if isinstance(item, str):
        return {
            "title": None,
            "url": item,
            "canonical_url": None,
            "meta": None,
            "children": [],
            "is_index": _is_index(item),
            "active": False,
        }

    # Handle Title: URL
    if isinstance(item, dict):
        for title, value in item.items():
            if isinstance(value, str):
                return {
                    "title": str(title),
                    "url": value.strip(),
                    "canonical_url": None,
                    "meta": None,
                    "children": [],
                    "is_index": _is_index(value.strip()),
                    "active": False,
                }
            if isinstance(value, list):
                return {
                    "title": str(title),
                    "url": None,
                    "canonical_url": None,
                    "meta": None,
                    "children": [_convert_nav_item(child) for child in value],
                    "is_index": False,
                    "active": False,
                }
            raise ConfigurationError(
                f"Unknown nav item value type: {type(value)}"
            )

    # Handle a list of items
    elif isinstance(item, list):
        return [_convert_nav_item(child) for child in item]

    raise ConfigurationError(f"Unknown nav item type: {type(item)}")


def _convert_extra(data: dict | list) -> dict | list:
    """Recursively convert None values in a dictionary/list to empty strings."""
    if isinstance(data, dict):
        # Process each key-value pair in the dictionary
        return {
            key: _convert_extra(value)
            if isinstance(value, (dict, list))
            else ("" if value is None else value)
            for key, value in data.items()
        }
    if isinstance(data, list):
        # Process each item in the list
        return [
            _convert_extra(item)
            if isinstance(item, (dict, list))
            else ("" if item is None else item)
            for item in data
        ]
    return data


def _convert_extra_javascript(value: list) -> list:
    """Ensure extra_javascript uses a structured format."""
    for i, item in enumerate(value):
        if isinstance(item, str):
            value[i] = {
                "path": item,
                "type": None,
                "async": False,
                "defer": False,
            }
        elif isinstance(item, dict):
            item.setdefault("path", "")
            item.setdefault("type", None)
            item.setdefault("async", False)
            item.setdefault("defer", False)
        else:
            raise ConfigurationError(
                f"Unknown extra_javascript item type: {type(item)}"
            )

    # Return resulting value
    return value


def _convert_markdown_extensions(
    value: Any,
) -> tuple[list[str], dict[str, dict[str, Any]]]:
    """Convert Markdown extensions to what Python Markdown expects."""
    markdown_extensions = ["toc", "tables"]
    mdx_configs: dict[str, dict[str, Any]] = {"toc": {}, "tables": {}}

    if value is None:
        return markdown_extensions, mdx_configs
    if not isinstance(value, (dict, list)):
        raise ConfigurationError(
            "Markdown extensions must be a list or mapping"
        )

    # In case of Python Markdown Extensions, we allow to omit the necessary
    # quotes around the extension names, so we need to hoist the extensions
    # configuration one level up. This is a pre-processing step before we
    # actually parse the configuration.
    if isinstance(value, dict):
        value = dict(value)

    if isinstance(value, dict) and "pymdownx" in value:
        pymdownx = value.pop("pymdownx")
        if not isinstance(pymdownx, dict):
            raise ConfigurationError(
                "pymdownx Markdown extensions must be a mapping"
            )
        for ext, conf in pymdownx.items():
            if not isinstance(ext, str):
                raise ConfigurationError(
                    "Markdown extension names must be strings"
                )
            # Special case for blocks extension, which has another level of
            # nesting. This is the only extension that requires this.
            if ext == "blocks":
                if not isinstance(conf, dict):
                    raise ConfigurationError(
                        "pymdownx.blocks Markdown extensions must be a mapping"
                    )
                for block, config in conf.items():
                    if not isinstance(block, str):
                        raise ConfigurationError(
                            "Markdown extension names must be strings"
                        )
                    value[f"pymdownx.{ext}.{block}"] = config
            else:
                value[f"pymdownx.{ext}"] = conf

    # Same as for Python Markdown extensions, see above
    if isinstance(value, dict) and "zensical" in value:
        zensical = value.pop("zensical")
        if not isinstance(zensical, dict):
            raise ConfigurationError(
                "zensical Markdown extensions must be a mapping"
            )
        for ext, conf in zensical.items():
            if not isinstance(ext, str):
                raise ConfigurationError(
                    "Markdown extension names must be strings"
                )
            if ext == "extensions":
                if not isinstance(conf, dict):
                    raise ConfigurationError(
                        "zensical.extensions Markdown extensions must be a "
                        "mapping"
                    )
                for key, config in conf.items():
                    if not isinstance(key, str):
                        raise ConfigurationError(
                            "Markdown extension names must be strings"
                        )
                    value[f"zensical.{ext}.{key}"] = config
            else:
                value[f"zensical.{ext}"] = conf

    extensions, configs = _convert_plugin_markdown_extensions(value)
    markdown_extensions.extend(extensions)
    mdx_configs.update(configs)

    # Return extension list and configuration, after ensuring they're unique
    seen = set()
    mdx_exts = [
        ext for ext in markdown_extensions if not (ext in seen or seen.add(ext))
    ]
    return mdx_exts, mdx_configs


def _convert_plugin_markdown_extensions(
    value: Any,
) -> tuple[list[str], dict[str, dict[str, Any]]]:
    """Normalize a plugin-local Python-Markdown configuration.

    Unlike the site renderer, plugin-local Markdown parsers do not inherit
    Zensical's default extensions. This mirrors MkDocs' MarkdownExtensions
    configuration option while retaining extension names and configuration in
    Python, where callable values remain usable.
    """
    markdown_extensions: list[str] = []
    mdx_configs: dict[str, dict[str, Any]] = {}

    def add(extension: Any, extension_config: Any) -> None:
        if not isinstance(extension, str):
            raise ConfigurationError("Markdown extension names must be strings")
        if extension_config is None:
            extension_config = {}
        if not isinstance(extension_config, dict):
            raise ConfigurationError(
                "Markdown extension configurations must be mappings"
            )
        markdown_extensions.append(extension)
        mdx_configs[extension] = cast("dict[str, Any]", extension_config)

    if value is None:
        return markdown_extensions, mdx_configs
    if not isinstance(value, (dict, list)):
        raise ConfigurationError(
            "Plugin Markdown extensions must be a list or mapping"
        )

    if isinstance(value, dict):
        for extension, extension_config in value.items():
            add(extension, extension_config)
    else:
        for item in value:
            if isinstance(item, str):
                add(item, {})
                continue
            if not isinstance(item, dict):
                raise ConfigurationError(
                    "Markdown extensions must be strings or mappings"
                )
            if len(item) != 1:
                raise ConfigurationError(
                    "Markdown extension mappings must contain one entry"
                )
            extension, extension_config = next(iter(item.items()))
            add(extension, extension_config)
    return markdown_extensions, mdx_configs


def _reject_unknown_options(
    label: str, plugin: dict[str, Any], supported: set[str]
) -> None:
    """Reject unknown options in a supported plugin configuration."""
    for name in plugin:
        if not isinstance(name, str):
            raise ConfigurationError(f"{label} option names must be strings")
    if unknown := set(plugin) - supported:
        name = sorted(unknown)[0]
        raise ConfigurationError(f"unknown {label} option: {name}")


def _validate_boolean_options(
    label: str, plugin: dict[str, Any], options: Iterable[str]
) -> None:
    """Validate boolean options in a supported plugin configuration."""
    for name in options:
        if name in plugin and not isinstance(plugin[name], bool):
            raise ConfigurationError(f"{label} {name} must be a boolean")


def _validate_string_options(
    label: str, plugin: dict[str, Any], options: Iterable[str]
) -> None:
    """Validate string options in a supported plugin configuration."""
    for name in options:
        if name in plugin and not isinstance(plugin[name], str):
            raise ConfigurationError(f"{label} {name} must be a string")


def _convert_plugins(value: Any, config: dict) -> dict:
    """Convert plugins configuration to something we can work with."""
    plugins: dict[str, Any] = {}
    tags: list[dict[str, Any]] = []

    def add(name: Any, data: Any) -> None:
        """Canonicalize Material aliases while preserving tag instances."""
        if not isinstance(name, str):
            raise ConfigurationError("Plugin names must be strings")
        name = name.removeprefix("material/")
        if data is None:
            data = {}
        elif not isinstance(data, dict):
            raise ConfigurationError(f"{name} configuration must be a mapping")
        else:
            data = dict(data)
        if name == "tags":
            tags.append({"name": name, "config": data})
        else:
            plugins[name] = data

    if value is None:
        pass
    # Plugins can be defined as a dict
    elif isinstance(value, dict):
        for name, data in value.items():
            add(name, data)
    # Plugins can also be defined as a list
    elif isinstance(value, list):
        for item in value:
            if isinstance(item, str):
                add(item, {})
            elif isinstance(item, dict) and len(item) == 1:
                name, data = next(iter(item.items()))
                add(name, data)
            else:
                raise ConfigurationError(
                    "Plugin entries must be names or single-entry mappings"
                )
    else:
        raise ConfigurationError("plugins must be a list or mapping")

    # Rust owns all tags defaults, validation, scalar coercion and callable
    # lowering. Python only preserves ordered plugin instances and their raw
    # configuration, as it does for future native compatibility modules.
    plugins["tags"] = tags

    # Search is enabled by default, even when it isn't explicitly configured.
    search = plugins.pop("search", {})
    _reject_unknown_options("search", search, {"enabled", "separator"})
    set_default(search, "enabled", True)
    set_default(search, "separator", '[\\s\\-_,:!=\\[\\]()\\\\"`/]+|\\.(?!\\d)')
    _validate_boolean_options("search", search, ("enabled",))
    _validate_string_options("search", search, ("separator",))
    plugins["search"] = search

    # Normalize metadata into the typed Rust configuration. The Material
    # namespace is removed at admission, so both names share this code path.
    # The configuration is always materialized and enabled by plugin presence.
    present = "meta" in plugins
    meta = plugins.pop("meta", {})
    _reject_unknown_options("meta", meta, {"enabled", "meta_file"})
    set_default(meta, "enabled", present)
    set_default(meta, "meta_file", ".meta.yml")
    _validate_boolean_options("meta", meta, ("enabled",))
    _validate_string_options("meta", meta, ("meta_file",))
    plugins["meta"] = meta

    # Normalize redirects into typed native configuration. The enabled flag is
    # internal; plugin presence retains MkDocs' activation semantics. The
    # configuration is always materialized for Rust.
    present = "redirects" in plugins
    redirects = plugins.pop("redirects", {})
    _reject_unknown_options(
        "redirects", redirects, {"enabled", "redirect_maps"}
    )
    set_default(redirects, "enabled", present)
    _validate_boolean_options("redirects", redirects, ("enabled",))
    set_default(redirects, "redirect_maps", {})
    redirect_maps = redirects["redirect_maps"]
    if not isinstance(redirect_maps, dict) or not all(
        isinstance(source, str) and isinstance(target, str)
        for source, target in redirect_maps.items()
    ):
        raise ConfigurationError(
            "redirects redirect_maps must be a mapping of strings to strings"
        )
    plugins["redirects"] = redirects

    # Normalize the complete mkdocs-minify-plugin configuration surface. Asset
    # settings are retained for the dedicated copy/output stage; the inline
    # switches are Zensical extensions handled by the final HTML pass. The
    # configuration is always materialized and enabled by plugin presence.
    present = "minify" in plugins
    minify = plugins.pop("minify", {})
    boolean_defaults = {
        "enabled": present,
        "minify_html": False,
        "minify_js": False,
        "minify_css": False,
        "minify_inline_js": False,
        "minify_inline_css": False,
        "cache_safe": False,
    }
    _reject_unknown_options(
        "minify",
        minify,
        set(boolean_defaults)
        | {
            "js_files",
            "css_files",
            "htmlmin_opts",
        },
    )
    for name, default in boolean_defaults.items():
        set_default(minify, name, default)
    _validate_boolean_options("minify", minify, boolean_defaults)
    for name in ("js_files", "css_files"):
        set_default(minify, name, [])
        files = minify[name]
        if isinstance(files, str):
            minify[name] = [files]
        elif not isinstance(files, list) or not all(
            isinstance(file, str) for file in files
        ):
            raise ConfigurationError(
                f"minify {name} must be a string or a list of strings"
            )

    set_default(minify, "htmlmin_opts", {})
    if not isinstance(minify["htmlmin_opts"], dict):
        raise ConfigurationError("minify htmlmin_opts must be a mapping")
    htmlmin_opts = dict(minify["htmlmin_opts"])
    boolean_defaults = {
        "remove_comments": False,
        "remove_empty_space": False,
        "remove_all_empty_space": False,
        "reduce_empty_attributes": True,
        "reduce_boolean_attributes": False,
        "remove_optional_attribute_quotes": True,
        "convert_charrefs": True,
        "keep_pre": False,
    }
    _reject_unknown_options(
        "minify htmlmin_opts",
        htmlmin_opts,
        set(boolean_defaults) | {"pre_tags", "pre_attr"},
    )
    for name, default in boolean_defaults.items():
        set_default(htmlmin_opts, name, default)
    _validate_boolean_options(
        "minify htmlmin_opts", htmlmin_opts, boolean_defaults
    )
    set_default(htmlmin_opts, "pre_tags", ["pre", "textarea"])
    if not isinstance(htmlmin_opts["pre_tags"], list) or not all(
        isinstance(tag, str) for tag in htmlmin_opts["pre_tags"]
    ):
        raise ConfigurationError(
            "minify htmlmin_opts pre_tags must be a list of strings"
        )
    set_default(htmlmin_opts, "pre_attr", "pre")
    _validate_string_options("minify htmlmin_opts", htmlmin_opts, ("pre_attr",))
    minify["htmlmin_opts"] = htmlmin_opts
    plugins["minify"] = minify

    # Normalize mkdocs-literate-nav without importing or executing the plugin.
    # Python retains extension objects and callables for the narrow Markdown
    # rendering boundary; Rust owns discovery and navigation resolution. The
    # configuration is always materialized and enabled by plugin presence, and
    # its Markdown settings belong to its private parser.
    present = "literate-nav" in plugins
    literate_nav = plugins.pop("literate-nav", {})
    _reject_unknown_options(
        "literate-nav",
        literate_nav,
        {
            "enabled",
            "nav_file",
            "implicit_index",
            "tab_length",
            "markdown_extensions",
        },
    )
    set_default(literate_nav, "enabled", present)
    set_default(literate_nav, "nav_file", "SUMMARY.md")
    set_default(literate_nav, "implicit_index", False)
    set_default(literate_nav, "tab_length", 4)
    _validate_boolean_options(
        "literate-nav", literate_nav, ("enabled", "implicit_index")
    )
    _validate_string_options("literate-nav", literate_nav, ("nav_file",))
    if (
        not isinstance(literate_nav["tab_length"], int)
        or isinstance(literate_nav["tab_length"], bool)
        or literate_nav["tab_length"] <= 0
    ):
        raise ConfigurationError(
            "literate-nav tab_length must be a positive integer"
        )

    set_default(literate_nav, "markdown_extensions", [])
    extensions, extension_configs = _convert_plugin_markdown_extensions(
        literate_nav["markdown_extensions"]
    )
    literate_nav["markdown_extensions"] = extensions
    literate_nav["mdx_configs"] = extension_configs
    plugins["literate_nav"] = literate_nav

    # Normalize mkdocs-awesome-nav without importing or executing the plugin.
    # Rust owns discovery, YAML parsing, matching and navigation resolution.
    # The configuration is always materialized and enabled by plugin presence.
    present = "awesome-nav" in plugins
    awesome_nav = plugins.pop("awesome-nav", {})
    _reject_unknown_options(
        "awesome-nav", awesome_nav, {"enabled", "filename", "logs"}
    )
    set_default(awesome_nav, "enabled", present)
    _validate_boolean_options("awesome-nav", awesome_nav, ("enabled",))
    set_default(awesome_nav, "filename", ".nav.yml")
    if (
        not isinstance(awesome_nav["filename"], str)
        or not awesome_nav["filename"]
    ):
        raise ConfigurationError(
            "awesome-nav filename must be a non-empty string"
        )
    set_default(awesome_nav, "logs", {})
    if not isinstance(awesome_nav["logs"], dict):
        raise ConfigurationError("awesome-nav logs must be a mapping")
    logs = dict(awesome_nav["logs"])
    log_names = ("nav_override", "root_title", "root_hide", "no_matches")
    _reject_unknown_options("awesome-nav log", logs, set(log_names))
    for name in log_names:
        set_default(logs, name, None)
        if logs[name] not in (None, "info", "warning", "error"):
            raise ConfigurationError(
                f"awesome-nav log {name} must be info, warning or error "
                "(or null)"
            )
    awesome_nav["logs"] = logs
    plugins["awesome_nav"] = awesome_nav

    # Offline is always materialized for Rust and enabled by plugin presence.
    present = "offline" in plugins
    offline = plugins.pop("offline", {})
    _reject_unknown_options("offline", offline, {"enabled"})
    set_default(offline, "enabled", present)
    _validate_boolean_options("offline", offline, ("enabled",))
    plugins["offline"] = offline

    # Mike signals versioned builds through an environment variable. During
    # one, we read its plugin configuration and mirror Mike's site URL
    # adjustment; support is currently limited to our theme. Explicit plugin
    # configurations are also normalized outside a versioned build.
    version = os.environ.get("MIKE_DOCS_VERSION")
    if "mike" in plugins or (version and config.get("site_url")):
        mike = plugins.pop("mike", {})
        string_defaults = {
            "alias_type": "symlink",
            "deploy_prefix": "",
        }
        nullable_strings = ("redirect_template", "canonical_version")
        _reject_unknown_options(
            "mike",
            mike,
            {"enabled", *string_defaults, *nullable_strings},
        )
        _validate_boolean_options("mike", mike, ("enabled",))
        for name, default in string_defaults.items():
            set_default(mike, name, default)
        _validate_string_options("mike", mike, string_defaults)
        for name in nullable_strings:
            set_default(mike, name, None)
            if mike[name] is not None and not isinstance(mike[name], str):
                raise ConfigurationError(
                    f"mike {name} must be a string or null"
                )
        plugins["mike"] = mike

    # Validate settings forwarded by the plugin-to-extension shims.
    if "autorefs" in plugins:
        autorefs = plugins["autorefs"]
        _reject_unknown_options("autorefs", autorefs, {"enabled"})
        _validate_boolean_options("autorefs", autorefs, ("enabled",))

    if "markdown-exec" in plugins:
        markdown_exec = plugins["markdown-exec"]
        _reject_unknown_options(
            "markdown-exec", markdown_exec, {"enabled", "ansi", "languages"}
        )
        _validate_boolean_options("markdown-exec", markdown_exec, ("enabled",))
        if "ansi" in markdown_exec:
            ansi = markdown_exec["ansi"]
            if not isinstance(ansi, bool) and ansi not in {
                "auto",
                "off",
                "required",
            }:
                raise ConfigurationError(
                    "markdown-exec ansi must be 'auto', 'off', 'required', "
                    "true or false"
                )
        if "languages" in markdown_exec:
            languages = markdown_exec["languages"]
            supported = {
                "bash",
                "console",
                "md",
                "markdown",
                "py",
                "python",
                "pycon",
                "pyodide",
                "sh",
                "tree",
            }
            if languages is not None and (
                not isinstance(languages, list)
                or not all(
                    isinstance(language, str) and language in supported
                    for language in languages
                )
            ):
                raise ConfigurationError(
                    "markdown-exec languages must be a list of supported "
                    "language names or null"
                )

    if "mkdocstrings" in plugins:
        mkdocstrings = plugins["mkdocstrings"]
        string_options = {"default_handler", "locale"}
        _reject_unknown_options(
            "mkdocstrings",
            mkdocstrings,
            string_options
            | {
                "enabled",
                "handlers",
                "custom_templates",
                "enable_inventory",
            },
        )
        _validate_boolean_options("mkdocstrings", mkdocstrings, ("enabled",))
        if (
            "handlers" in mkdocstrings
            and mkdocstrings["handlers"] is not None
            and not isinstance(mkdocstrings["handlers"], dict)
        ):
            raise ConfigurationError(
                "mkdocstrings handlers must be a mapping or null"
            )
        if (
            "custom_templates" in mkdocstrings
            and mkdocstrings["custom_templates"] is not None
            and not isinstance(mkdocstrings["custom_templates"], str)
        ):
            raise ConfigurationError(
                "mkdocstrings custom_templates must be a string or null"
            )
        if (
            "enable_inventory" in mkdocstrings
            and mkdocstrings["enable_inventory"] is not None
            and not isinstance(mkdocstrings["enable_inventory"], bool)
        ):
            raise ConfigurationError(
                "mkdocstrings enable_inventory must be a boolean or null"
            )
        _validate_string_options("mkdocstrings", mkdocstrings, string_options)

    if "glightbox" in plugins:
        glightbox = plugins["glightbox"]
        string_options = {"width", "height", "background"}
        boolean_options = {
            "enabled",
            "auto",
            "auto_themed",
            "auto_caption",
            "touchNavigation",
            "loop",
            "zoomable",
            "draggable",
            "shadow",
        }
        _reject_unknown_options(
            "glightbox",
            glightbox,
            string_options
            | boolean_options
            | {
                "skip_classes",
                "caption_position",
                "effect",
                "manual",
            },
        )
        _validate_string_options("glightbox", glightbox, string_options)
        if "skip_classes" in glightbox and (
            not isinstance(glightbox["skip_classes"], list)
            or not all(
                isinstance(name, str) for name in glightbox["skip_classes"]
            )
        ):
            raise ConfigurationError(
                "glightbox skip_classes must be a list of strings"
            )
        _validate_boolean_options("glightbox", glightbox, boolean_options)
        if "caption_position" in glightbox and glightbox[
            "caption_position"
        ] not in {"bottom", "top", "left", "right"}:
            raise ConfigurationError(
                "glightbox caption_position must be 'bottom', 'top', 'left' "
                "or 'right'"
            )
        if "effect" in glightbox and glightbox["effect"] not in {
            "zoom",
            "fade",
            "none",
        }:
            raise ConfigurationError(
                "glightbox effect must be 'zoom', 'fade' or 'none'"
            )
        if (
            "manual" in glightbox
            and glightbox["manual"] is not None
            and not isinstance(glightbox["manual"], bool)
        ):
            raise ConfigurationError(
                "glightbox manual must be a boolean or null"
            )

    if "macros" in plugins:
        macros = plugins["macros"]
        string_options = {
            "module_name",
            "include_dir",
            "j2_block_start_string",
            "j2_block_end_string",
            "j2_variable_start_string",
            "j2_variable_end_string",
            "j2_comment_start_string",
            "j2_comment_end_string",
        }
        list_options = {"modules", "j2_extensions"}
        boolean_options = {
            "enabled",
            "render_by_default",
            "on_error_fail",
            "verbose",
        }
        _reject_unknown_options(
            "macros",
            macros,
            string_options
            | list_options
            | boolean_options
            | {
                "include_yaml",
                "on_undefined",
            },
        )
        _validate_string_options("macros", macros, string_options)
        for name in list_options:
            if name in macros and (
                not isinstance(macros[name], list)
                or not all(isinstance(item, str) for item in macros[name])
            ):
                raise ConfigurationError(
                    f"macros {name} must be a list of strings"
                )
        _validate_boolean_options("macros", macros, boolean_options)
        if "include_yaml" in macros:
            include_yaml = macros["include_yaml"]
            valid_list = isinstance(include_yaml, list) and all(
                isinstance(path, str) for path in include_yaml
            )
            valid_mapping = isinstance(include_yaml, dict) and all(
                isinstance(name, str) and isinstance(path, str)
                for name, path in include_yaml.items()
            )
            if not valid_list and not valid_mapping:
                raise ConfigurationError(
                    "macros include_yaml must be a list of strings or a "
                    "mapping of strings to strings"
                )
        if "on_undefined" in macros and macros["on_undefined"] not in {
            "keep",
            "strict",
        }:
            raise ConfigurationError(
                "macros on_undefined must be 'keep' or 'strict'"
            )

    if "table-reader" in plugins:
        table_reader = plugins["table-reader"]
        _reject_unknown_options(
            "table-reader",
            table_reader,
            {
                "enabled",
                "data_path",
                "allow_missing_files",
                "select_readers",
            },
        )
        _validate_boolean_options(
            "table-reader", table_reader, ("enabled", "allow_missing_files")
        )
        _validate_string_options("table-reader", table_reader, ("data_path",))

        if "data_path" in table_reader:
            root = Path(config["root_dir"]).resolve()
            data_path = Path(table_reader["data_path"])
            if not data_path.is_absolute():
                data_path = root / data_path
            if not data_path.resolve().is_relative_to(root):
                raise ConfigurationError(
                    "table-reader data_path must be within project root"
                )

        if "select_readers" in table_reader:
            selected = table_reader["select_readers"]
            if not isinstance(selected, list) or not all(
                isinstance(reader, str) for reader in selected
            ):
                raise ConfigurationError(
                    "table-reader select_readers must be a list of reader names"
                )
            if unsupported := set(selected) - set(TABLE_READERS):
                reader = sorted(unsupported)[0]
                raise ConfigurationError(
                    f"unknown table-reader reader: {reader}"
                )

    # Add one level of indirection by moving every plugin configuration into a
    # `config` property, matching Material's template-facing representation.
    wrapped = {name: {"config": plugin} for name, plugin in plugins.items()}
    _apply_offline_plugin(config, wrapped)
    _apply_mike_plugin(config, wrapped, version)
    return wrapped


def _apply_offline_plugin(config: dict, plugins: dict[str, Any]) -> None:
    """Apply project-wide effects of the offline plugin."""
    offline = plugins["offline"]["config"]
    if not offline["enabled"]:
        return

    # Ensure correct resolution of links when viewing the site from the file
    # system by disabling directory URLs.
    config["use_directory_urls"] = False

    # Append iframe-worker to polyfills/shims.
    if not any(
        "iframe-worker"
        in (url.get("path", "") if isinstance(url, dict) else url)
        for url in config["extra"]["polyfills"]
    ):
        config["extra"]["polyfills"].append(
            {
                "path": "https://unpkg.com/iframe-worker/shim",
                "type": "text/javascript",
                "async": False,
                "defer": False,
            }
        )


def _apply_mike_plugin(
    config: dict,
    plugins: dict[str, Any],
    version: str | None,
) -> None:
    """Apply project-wide effects of a mike versioned build."""
    if not version or not config.get("site_url"):
        return
    mike = plugins["mike"]["config"]

    # Copied and adapted from Mike's plugin implementation.
    version = mike["canonical_version"] or version
    config["site_url"] = urljoin(config["site_url"], version)
