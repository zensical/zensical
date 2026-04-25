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
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator

# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------

_RE = re.compile(
    r"""
    # Any element with an `id` attribute:
    #
    #   <h2 id="section-1">
    #
    (?P<id>
        <[a-zA-Z][^>]*\sid=["'](?P<id_value>[^"']+)["'][^>]*>
    )
    |
    # Anchor elements with `name` attribute:
    #
    #   <a name="top">
    #
    (?P<name>
        <a\s[^>]*name=["'](?P<name_value>[^"']+)["'][^>]*>
    )
    """,
    re.VERBOSE | re.IGNORECASE,
)
"""
Match anchor declarations in rendered HTML.
"""

# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def anchors(html: str) -> Iterator[str]:
    """Scan HTML and yield anchor declarations."""
    for match in _RE.finditer(html):
        value = match.group("id_value") or match.group("name_value")
        if value:
            yield value
