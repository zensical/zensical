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

"""Test the URLs advertised by the preview server."""

from __future__ import annotations

import re
import socket
import subprocess
import sys
import time
from typing import TYPE_CHECKING
from urllib.parse import urlsplit
from urllib.request import ProxyHandler, build_opener

import pytest

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize(
    "dev_addr",
    ["127.0.0.1:0", "127.0.0.1:00", "[::1]:0", "localhost:0"],
)
@pytest.mark.parametrize("from_config", [False, True])
@pytest.mark.parametrize("open_browser", [False, True])
def test_serve_advertises_reachable_ephemeral_ports(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    dev_addr: str,
    from_config: bool,
    open_browser: bool,
) -> None:
    """Connect to every URL advertised for an ephemeral port."""
    host = dev_addr.rsplit(":", 1)[0].strip("[]")
    resolved = socket.getaddrinfo(host, 0, type=socket.SOCK_STREAM)
    expected_hosts = {addr[0] for _, _, _, _, addr in resolved}

    # The IPv6 cases are skipped if the loopback interface is unavailable.
    if "::1" in expected_hosts:
        try:
            with socket.socket(socket.AF_INET6, socket.SOCK_STREAM) as probe:
                probe.bind(("::1", 0))
        except OSError:
            pytest.skip("IPv6 loopback is unavailable")

    # The address is supplied through either configuration or the CLI.
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Ephemeral port test\n", encoding="utf-8")
    config = tmp_path / "zensical.toml"
    configured_addr = dev_addr if from_config else "127.0.0.1:8000"
    config.write_text(
        f"""[project]
site_name = "Test"
dev_addr = "{configured_addr}"
""",
        encoding="utf-8",
    )
    command = [
        sys.executable,
        "-m",
        "zensical",
        "serve",
        "--config-file",
        str(config),
    ]
    if not from_config:
        command.extend(["--dev-addr", dev_addr])

    # On Linux, browser launches can be captured through the BROWSER variable.
    opened_url = tmp_path / "opened-url"
    if open_browser:
        if sys.platform != "linux":
            pytest.skip("Browser capture requires Linux")
        browser = tmp_path / "browser"
        browser.write_text(
            r"""#!/bin/sh
printf '%s\n' "$1" > "$ZENSICAL_TEST_OPENED_URL"
""",
            encoding="utf-8",
        )
        browser.chmod(0o755)
        monkeypatch.setenv("BROWSER", str(browser))
        monkeypatch.setenv("ZENSICAL_TEST_OPENED_URL", str(opened_url))
        command.append("--open")

    # Output is captured until the initial page has been written.
    log_path = tmp_path / "serve.log"
    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(  # noqa: S603
            command, cwd=tmp_path, stdout=log, stderr=subprocess.STDOUT
        )
        try:
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                output = log_path.read_text(encoding="utf-8")
                page_ready = (tmp_path / "site" / "index.html").is_file()
                browser_ready = not open_browser or opened_url.is_file()
                if page_ready and browser_ready:
                    break
                assert process.poll() is None, output
                time.sleep(0.02)
            else:
                pytest.fail(f"Server did not finish building:\n{output}")

            # Every resolved address must have a concrete, reachable URL.
            urls = re.findall(
                r"^Serving .+ on (http://\S+)$", output, re.MULTILINE
            )
            assert len(urls) == len(expected_hosts), output
            assert {urlsplit(url).hostname for url in urls} == expected_hosts
            if open_browser:
                # A trailing slash may be added by the browser launcher.
                browser_url = opened_url.read_text(encoding="utf-8").strip()
                assert browser_url.rstrip("/") == urls[0]
            opener = build_opener(ProxyHandler({}))
            for url in urls:
                assert urlsplit(url).port != 0
                with opener.open(url, timeout=5) as response:
                    assert response.status == 200
                    assert b"Ephemeral port test" in response.read()
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
