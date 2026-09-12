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

"""Test when mkdocstrings writes `objects.inv`, using fake handlers."""

from types import SimpleNamespace

import pytest

import zensical.config as config_module
from zensical.compat import mkdocstrings


@pytest.mark.parametrize(
    ("setting", "handlers", "cached", "expected", "automatic"),
    [
        (None, None, False, False, False),
        (None, None, True, True, True),
        (None, [], False, False, False),
        (None, [False], False, False, False),
        (None, [False, True], False, True, True),
        (None, [False], True, True, True),
        (True, [False], False, True, False),
        (True, None, False, True, False),
        (False, [True], False, False, True),
        (False, None, True, False, True),
    ],
)
def test_inventory_policy_combines_settings_with_cached_handlers(
    monkeypatch: pytest.MonkeyPatch,
    setting: bool | None,
    handlers: list[bool] | None,
    cached: bool,
    expected: bool,
    automatic: bool,
) -> None:
    monkeypatch.setattr(
        config_module,
        "_CONFIG",
        {
            "plugins": {
                "mkdocstrings": {"config": {"enable_inventory": setting}}
            }
        },
    )
    monkeypatch.setattr(mkdocstrings, "_ENABLE_INVENTORY", setting)
    monkeypatch.setattr(
        mkdocstrings,
        "HANDLERS",
        (
            None
            if handlers is None
            else SimpleNamespace(
                seen_handlers=[
                    SimpleNamespace(enable_inventory=value)
                    for value in handlers
                ]
            )
        ),
    )
    assert mkdocstrings.get_inventory_policy(cached) == (expected, automatic)


@pytest.mark.parametrize("as_extension", [False, True])
def test_disabled_mkdocstrings_does_not_export_cached_inventory(
    monkeypatch: pytest.MonkeyPatch, as_extension: bool
) -> None:
    options = {"enabled": False, "enable_inventory": True}
    config = (
        {"mdx_configs": {"zensical.extensions.mkdocstrings": options}}
        if as_extension
        else {"plugins": {"mkdocstrings": {"config": options}}}
    )
    monkeypatch.setattr(config_module, "_CONFIG", config)
    monkeypatch.setattr(mkdocstrings, "HANDLERS", None)
    assert mkdocstrings.get_inventory_policy(True) == (False, True)


def test_cached_inventory_uses_explicit_extension_options(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        config_module,
        "_CONFIG",
        {
            "plugins": {"mkdocstrings": {"config": {"enable_inventory": True}}},
            "mdx_configs": {
                "zensical.extensions.mkdocstrings": {"enable_inventory": False}
            },
        },
    )
    monkeypatch.setattr(mkdocstrings, "HANDLERS", None)
    assert mkdocstrings.get_inventory_policy(True) == (False, True)
