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

from __future__ import annotations

import hashlib
import importlib
import os
import pickle
import yaml

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib  # type: ignore

from click import ClickException
from deepmerge import always_merger
from typing import Any, IO
from yaml import BaseLoader, Loader, YAMLError
from yaml.constructor import ConstructorError
from urllib.parse import urlparse

from .extensions.emoji import to_svg, twemoji

# ----------------------------------------------------------------------------
# Globals
# ----------------------------------------------------------------------------


_CONFIG = None
"""
Global configuration to pick up later for parsing Markdown.

Since MkDocs uses YAML as a configuration format, the configuration can contain
references to functions or other Python objects, for which we don't have any
representation in Rust. Thus, we just keep the configuration on the Python
side, and use it directly when needed. It's a hack but will do for now.
"""

# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class ConfigurationError(ClickException):
    """
    Configuration resolution or validation failed.
    """


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def parse_config(path: str) -> dict:
    """
    Parse configuration file.
    """
    # Decide by extension; no need to convert to Path
    _, ext = os.path.splitext(path)
    if ext.lower() == ".toml":
        return parse_zensical_config(path)
    else:
        return parse_mkdocs_config(path)


def parse_zensical_config(path: str) -> dict:
    """
    Parse zensical.toml configuration file.
    """
    global _CONFIG
    with open(path, "rb") as f:
        config = tomllib.load(f)
    if "project" in config:
        config = config["project"]

    # Apply defaults and return parsed configuration
    _CONFIG = _apply_defaults(config, path)
    return _CONFIG


def parse_mkdocs_config(path: str) -> dict:
    """
    Parse mkdocs.yml configuration file.
    """
    global _CONFIG
    with open(path, "r") as f:
        config = _yaml_load(f)

    # Apply defaults and return parsed configuration
    _CONFIG = _apply_defaults(config, path)
    return _CONFIG


def get_config():
    """
    Return configuration.
    """
    return _CONFIG


def get_theme_dir() -> str:
    """
    Return the theme directory.
    """
    path = os.path.dirname(os.path.abspath(__file__))
    return os.path.join(path, "templates")


def _apply_defaults(config: dict, path: str) -> dict:
    """
    Apply default settings in configuration.

    Note that this is loosely based on the defaults that MkDocs sets in its own
    configuration system, which we won't port for compatibility right now, as
    well as the defaults that are set in Material for MkDocs for theme- and
    extra-level settings.

    We must set all properties, as well as nested properties to `None`, or PyO3
    will refuse to convert them, as the key must definitely exist.
    """
    if "site_name" not in config:
        raise ConfigurationError("Missing required setting: site_name")

    # Set site directory
    set_default(config, "site_dir", "site", str)
    if ".." in config.get("site_dir"):
        raise ConfigurationError("site_dir must not contain '..'")

    # Set docs directory
    set_default(config, "docs_dir", "docs", str)
    if ".." in config.get("docs_dir"):
        raise ConfigurationError("docs_dir must not contain '..'")

    # Set defaults for core settings
    set_default(config, "site_url", None, str)
    set_default(config, "site_description", None, str)
    set_default(config, "site_author", None, str)
    set_default(config, "use_directory_urls", True, bool)
    set_default(config, "dev_addr", "localhost:8000", str)
    set_default(config, "copyright", None, str)

    # Set defaults for repository settings
    set_default(config, "repo_url", None, str)
    set_default(config, "repo_name", None, str)
    set_default(config, "edit_uri_template", None, str)
    set_default(config, "edit_uri", None, str)

    # Set defaults for repository name settings
    repo_url = config.get("repo_url")
    if repo_url and not config.get("repo_name"):
        docs_dir = config.get("docs_dir")
        host = urlparse(repo_url).hostname or ""
        if host == "github.com":
            set_default(config, "repo_name", "GitHub", str)
            set_default(config, "edit_uri", f"edit/master/{docs_dir}", str)
        elif host == "gitlab.com":
            set_default(config, "repo_name", "GitLab", str)
            set_default(config, "edit_uri", f"edit/master/{docs_dir}", str)
        elif host == "bitbucket.org":
            set_default(config, "repo_name", "Bitbucket", str)
            set_default(config, "edit_uri", f"src/default/{docs_dir}", str)
        elif host:
            config["repo_name"] = host.split(".")[0].title()

    # Remove trailing slash from edit_uri if present
    edit_uri = config.get("edit_uri")
    if isinstance(edit_uri, str) and edit_uri.endswith("/"):
        config["edit_uri"] = edit_uri.rstrip("/")

    # Set defaults for theme font settings
    theme = set_default(config, "theme", {}, dict)
    if isinstance(theme, str):
        theme = {"name": theme}
        config["theme"] = theme

    # Set variant and fonts for variant
    set_default(theme, "variant", "modern", str)
    if theme.get("variant") == "modern":
        font = {"text": "Inter", "code": "JetBrains Mono"}
    else:
        font = {"text": "Roboto", "code": "Roboto Mono"}

    # Resolve custom theme directory
    set_default(theme, "custom_dir", None, str)
    if theme.get("custom_dir"):
        theme["custom_dir"] = os.path.join(
            os.path.dirname(path), theme["custom_dir"]
        )

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
    set_default(admonition, "note", None, str)
    set_default(admonition, "abstract", None, str)
    set_default(admonition, "info", None, str)
    set_default(admonition, "tip", None, str)
    set_default(admonition, "success", None, str)
    set_default(admonition, "question", None, str)
    set_default(admonition, "warning", None, str)
    set_default(admonition, "failure", None, str)
    set_default(admonition, "danger", None, str)
    set_default(admonition, "bug", None, str)
    set_default(admonition, "example", None, str)
    set_default(admonition, "quote", None, str)

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
    extra = set_default(config, "extra", {}, dict)
    set_default(extra, "homepage", None, str)
    set_default(extra, "scope", None, str)
    set_default(extra, "annotate", {}, dict)
    set_default(extra, "tags", {}, dict)
    set_default(extra, "generator", True, bool)
    set_default(extra, "polyfills", [], list)
    set_default(extra, "analytics", None, dict)

    # Set defaults for extra analytics settings
    analytics = extra.get("analytics")
    if analytics:
        set_default(analytics, "provider", None, str)
        set_default(analytics, "property", None, str)
        set_default(analytics, "feedback", None, dict)

        # Set defaults for extra analytics feedback settings
        feedback = analytics.get("feedback")
        if feedback:
            set_default(feedback, "title", None, str)
            set_default(feedback, "ratings", [], list)

            # Set defaults for each rating entry
            ratings = feedback.setdefault("ratings", [])
            for entry in ratings:
                set_default(entry, "icon", None, str)
                set_default(entry, "name", None, str)
                set_default(entry, "data", None, str)
                set_default(entry, "note", None, str)

    # Set defaults for extra consent settings
    consent = extra.setdefault("consent", None)
    if consent:
        set_default(consent, "title", None, str)
        set_default(consent, "description", None, str)
        set_default(consent, "actions", [], list)

        # Set defaults for extra consent cookie settings
        cookies = consent.setdefault("cookies", {})
        for key, value in cookies.items():
            if isinstance(value, str):
                cookies[key] = {"name": value, "checked": False}

            # Set defaults for each cookie entry
            set_default(cookies[key], "name", None, str)
            set_default(cookies[key], "checked", False, bool)

    # Set defaults for extra social settings
    social = extra.setdefault("social", [])
    for entry in social:
        set_default(entry, "icon", None, str)
        set_default(entry, "name", None, str)
        set_default(entry, "link", None, str)

    # Set defaults for extra alternate settings
    alternate = extra.setdefault("alternate", [])
    for entry in alternate:
        set_default(entry, "name", None, str)
        set_default(entry, "link", None, str)
        set_default(entry, "lang", None, str)

    # Set defaults for extra version settings
    version = extra.setdefault("version", None)
    if version:
        set_default(version, "provider", None, str)
        set_default(version, "default", None, str)
        set_default(version, "alias", False, bool)

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

    # MkDocs will also set fenced_code, which is incompatible with SuperFences,
    # the extension that Material for MkDocs generally recommends. Note that we
    # decided to set defaults that make it easy to get started with sensible
    # Markdown support, but users can override this as needed.
    markdown_extensions, mdx_configs = _convert_markdown_extensions(
        config.get(
            "markdown_extensions",
            {
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
            },
        )
    )
    config["markdown_extensions"] = markdown_extensions
    config["mdx_configs"] = mdx_configs

    # Now, since YAML supports using Python tags to resolve functions, we need
    # to support the same for when we load TOML. This is a bandaid, and we will
    # find a better solution, once we work on configuration management, but for
    # now this should be sufficient.
    emoji = config["mdx_configs"].get("pymdownx.emoji", {})
    if isinstance(emoji.get("emoji_generator"), str):
        emoji["emoji_generator"] = _resolve(emoji.get("emoji_generator"))
    if isinstance(emoji.get("emoji_index"), str):
        emoji["emoji_index"] = _resolve(emoji.get("emoji_index"))

    # Tabbed extension configuration - resolve slugification function
    tabbed = config["mdx_configs"].get("pymdownx.tabbed", {})
    if isinstance(tabbed.get("slugify"), dict):
        object = tabbed["slugify"].get("object", "pymdownx.slugs.slugify")
        tabbed["slugify"] = _resolve(object)(**tabbed["slugify"].get("kwds"))

    # Table of contents extension configuration - resolve slugification function
    toc = config["mdx_configs"]["toc"]
    if isinstance(toc.get("slugify"), dict):
        object = toc["slugify"].get("object", "pymdownx.slugs.slugify")
        toc["slugify"] = _resolve(object)(**toc["slugify"].get("kwds"))

    # Superfences extension configuration - resolve format function
    superfences = config["mdx_configs"].get("pymdownx.superfences", {})
    for fence in superfences.get("custom_fences", []):
        if isinstance(fence.get("format"), str):
            fence["format"] = _resolve(fence.get("format"))

    # Ensure the table of contents title is initialized, as it's used inside
    # the template, and the table of contents extension is always defined
    config["mdx_configs"]["toc"].setdefault("title", None)
    config["mdx_configs_hash"] = _hash(mdx_configs)

    # Convert plugins configuration
    config["plugins"] = _convert_plugins(config.get("plugins", []), config)

    # mkdocstrings configuration
    if "mkdocstrings" in config["plugins"]:
        mkdocstrings_config = config["plugins"]["mkdocstrings"]["config"]
        if mkdocstrings_config.pop("enabled", True):
            mkdocstrings_config["markdown_extensions"] = [
                {ext: mdx_configs.get(ext, {})} for ext in markdown_extensions
            ]
            config["markdown_extensions"].append("mkdocstrings")
            config["mdx_configs"]["mkdocstrings"] = mkdocstrings_config

    return config


def set_default(
    entry: dict, key: str, default: Any, data_type: type | None = None
) -> Any:
    """
    Set a key to a default value if it isn't set, and optionally cast it to the specified data type.
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
            raise ValueError(f"Failed to cast key '{key}' to {data_type}: {e}")

    # Return the resulting value
    return entry[key]


def _hash(data: Any) -> int:
    """
    Compute a hash for the given data.
    """
    hash = hashlib.sha1(pickle.dumps(data))
    return int(hash.hexdigest(), 16) % (2**64)


def _convert_extra(data: dict | list) -> dict | list:
    """
    Recursively convert all None values in a dictionary or list to empty strings.
    """
    if isinstance(data, dict):
        # Process each key-value pair in the dictionary
        return {
            key: _convert_extra(value)
            if isinstance(value, (dict, list))
            else ("" if value is None else value)
            for key, value in data.items()
        }
    elif isinstance(data, list):
        # Process each item in the list
        return [
            _convert_extra(item)
            if isinstance(item, (dict, list))
            else ("" if item is None else item)
            for item in data
        ]
    else:
        return data


def _resolve(symbol: str):
    """
    Resolve a symbol to its corresponding Python object.
    """
    module_path, func_name = symbol.rsplit(".", 1)
    module = importlib.import_module(module_path)
    return getattr(module, func_name)


# -----------------------------------------------------------------------------


def _convert_nav(nav: list) -> list:
    """
    Convert MkDocs navigation
    """
    return [_convert_nav_item(entry) for entry in nav]


def _convert_nav_item(item: str | dict | list) -> dict | list:
    """
    Convert MkDocs shorthand navigation structure into something more manageable
    as we need to annotate each item with a title, URL, icon, and children.
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
    elif isinstance(item, dict):
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
            elif isinstance(value, list):
                return {
                    "title": str(title),
                    "url": None,
                    "canonical_url": None,
                    "meta": None,
                    "children": [_convert_nav_item(child) for child in value],
                    "is_index": False,
                    "active": False,
                }

    # Handle a list of items
    elif isinstance(item, list):
        return [_convert_nav_item(child) for child in item]
    else:
        raise ValueError(f"Unknown nav item type: {type(item)}")


def _is_index(path: str) -> bool:
    """
    Returns, whether the given path points to a section index.
    """
    return path.endswith(("index.md", "README.md"))


# -----------------------------------------------------------------------------


def _convert_extra_javascript(value: list[Any]) -> list:
    """
    Ensure extra_javascript uses a structured format.
    """
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
            raise ValueError(
                f"Unknown extra_javascript item type: {type(item)}"
            )

    # Return resulting value
    return value


# -----------------------------------------------------------------------------


def _convert_markdown_extensions(value: Any):
    """
    Convert Markdown extensions configuration to what Python Markdown expects.
    """
    markdown_extensions = ["toc", "tables"]
    mdx_configs = {"toc": {}, "tables": {}}

    # In case of Python Markdown Extensions, we allow to omit the necessary
    # quotes around the extension names, so we need to hoist the extensions
    # configuration one level up. This is a pre-processing step before we
    # actually parse the configuration.
    if "pymdownx" in value:
        pymdownx = value.pop("pymdownx")
        for ext, config in pymdownx.items():
            # Special case for blocks extension, which has another level of
            # nesting. This is the only extension that requires this.
            if ext == "blocks":
                for block, config in config.items():
                    value[f"pymdownx.{ext}.{block}"] = config
            else:
                value[f"pymdownx.{ext}"] = config

    # Same as for Python Markdown extensions, see above
    if "zensical" in value:
        zensical = value.pop("zensical")
        for ext, config in zensical.items():
            if ext == "extensions":
                for key, config in config.items():
                    value[f"zensical.{ext}.{key}"] = config
            else:
                value[f"zensical.{ext}"] = config

    # Extensions can be defined as a dict
    if isinstance(value, dict):
        for ext, config in value.items():
            markdown_extensions.append(ext)
            mdx_configs[ext] = config or {}

    # Extensions can also be defined as a list
    else:
        for item in value:
            if isinstance(item, dict):
                ext, config = item.popitem()
                markdown_extensions.append(ext)
                mdx_configs[ext] = config or {}
            elif isinstance(item, str):
                markdown_extensions.append(item)

    # Return extension list and configuration, after ensuring they're unique
    return list(set(markdown_extensions)), mdx_configs


# ----------------------------------------------------------------------------


def _convert_plugins(value: Any, config: dict) -> dict:
    """
    Convert plugins configuration to something we can work with.
    """
    plugins = {}

    # Plugins can be defined as a dict
    if isinstance(value, dict):
        for name, data in value.items():
            plugins[name] = data

    # Plugins can also be defined as a list
    else:
        for item in value:
            if isinstance(item, dict):
                name, data = item.popitem()
                plugins[name] = data
            elif isinstance(item, str):
                plugins[item] = {}

    # Define defaults for search plugin
    search = set_default(plugins, "search", {}, dict)
    set_default(search, "enabled", True, bool)
    set_default(
        search, "separator", '[\\s\\-_,:!=\\[\\]()\\\\"`/]+|\\.(?!\\d)', str
    )

    # Define defaults for offline plugin
    offline = set_default(plugins, "offline", {"enabled": False}, dict)
    set_default(offline, "enabled", True, bool)

    # Ensure correct resolution of links when viewing the site from the
    # file system by disabling directory URLs
    if offline.get("enabled"):
        config["use_directory_urls"] = False

        # Append iframe-worker to polyfills/shims
        if not any(
            "iframe-worker" in url for url in config["extra"]["polyfills"]
        ):
            script = "https://unpkg.com/iframe-worker/shim"
            config["extra"]["polyfills"].append(
                {
                    "path": script,
                    "type": "text/javascript",
                    "async": False,
                    "defer": False,
                }
            )

    # Now, add another level of indirection, by moving all plugin configuration
    # into a `config` property, making it compatible with Material for MkDocs.
    for name, data in plugins.items():
        if not isinstance(data, dict) or "config" not in data:
            plugins[name] = {"config": data}

    # Return plugins
    return plugins


# ----------------------------------------------------------------------------


def _yaml_load(
    source: IO, loader: type[BaseLoader] | None = None
) -> dict[str, Any]:
    """
    Load configuration file and resolve environment variables and parent files.

    Note that INHERIT is only a bandaid that was introduced to allow for some
    degree of modularity, but with serious shortcomings. Zensical will use a
    different approach in the future, which will allow for composable and
    environment-specific configuration.
    """
    loader = loader or Loader.add_constructor("!ENV", _construct_env_tag)
    try:
        config = yaml.load(
            # Compatibility shim: we remap Material's extension namespace to
            # Zensical's, and the now deprecated materialx namespace as well
            source.read()
            .replace("material.extensions", "zensical.extensions")
            .replace("materialx", "zensical.extensions"),
            Loader=Loader,
        )
    except YAMLError as e:
        raise ConfigurationError(
            f"Encountered an error parsing the configuration file: {e}"
        )
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
                f"Inherited config file '{relpath}' doesn't exist at '{abspath}'."
            )
        with open(abspath, "r") as fd:
            parent = _yaml_load(fd, loader)
        config = always_merger.merge(parent, config)

    # Return resulting configuration
    return config


def _construct_env_tag(loader: yaml.Loader, node: yaml.Node):
    """
    Assign value of ENV variable referenced at node.

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
            start_mark=node.start_mark,
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
