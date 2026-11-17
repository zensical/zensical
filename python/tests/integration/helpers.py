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
import time
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import subprocess
    from collections.abc import Callable


def wait_for_serve(
    condition: Callable[[], bool],
    process: subprocess.Popen,
    output: Callable[[], str],
    *,
    timeout: float = 30.0,
) -> None:
    """Wait for rendered output and validation from the current server run."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        messages = output()
        if process.poll() is not None:
            raise AssertionError(
                f"Serve process exited with {process.returncode}:\n{messages}"
            )

        # Pages and assets can be written before discovery has completed.
        # Validation is reported after rendering, and must belong to the
        # current run if a configuration or theme edit restarted the server.
        current_run = messages.rsplit("Build started", 1)[-1]
        summary = re.search(
            r"^(?:No issues found|\d+ issues? found)$",
            current_run,
            re.MULTILINE,
        )
        if summary:
            try:
                if condition():
                    return
            except FileNotFoundError:
                # Outputs can be replaced between existence checks and reads.
                pass

        time.sleep(0.02)

    raise AssertionError(f"Serve process timed out:\n{output()}")
