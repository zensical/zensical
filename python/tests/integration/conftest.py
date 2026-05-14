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

import copy
from typing import Any

import pytest

from zensical import config

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(name="base_config", scope="session")
def _fixture_base_config(
    tmp_path_factory: pytest.TempPathFactory,
) -> dict[str, Any]:
    """Build a fully-processed config once per session."""
    root = tmp_path_factory.mktemp("integration_base")
    (root / "docs").mkdir()
    return config._apply_defaults(
        {
            "site_name": "Test",
            "markdown_extensions": config.DEFAULT_MARKDOWN_EXTENSIONS,
        },
        str(root / "zensical.toml"),
    )


@pytest.fixture(autouse=True)
def _fixture_set_config(base_config: dict[str, Any]) -> Any:
    """Give each test a fresh copy of the config and restore state after.

    render() mutates the global config (it appends/updates ContextExtension),
    so every test must start from an isolated copy.
    """
    config._CONFIG = copy.deepcopy(base_config)
    yield
    config._CONFIG = None
