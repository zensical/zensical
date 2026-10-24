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

import hashlib
import json
import re
import threading
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import TYPE_CHECKING, Any
from urllib.parse import urljoin, urlsplit

import pytest
from bs4 import BeautifulSoup, Tag

import zensical

if TYPE_CHECKING:
    from pathlib import Path


@pytest.fixture
def remote() -> Any:
    """Serve deterministic assets and record every actual network request."""
    responses: dict[str, tuple[str, bytes]] = {}
    requests: Counter[str] = Counter()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            requests[self.path] += 1
            if self.path not in responses:
                self.send_error(404)
                return
            mime, body = responses[self.path]
            self.send_response(200)
            self.send_header("Content-Type", mime)
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format: str, *args: Any) -> None:  # noqa: A002
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", responses, requests
    finally:
        server.shutdown()
        server.server_close()
        worker.join()


def _project(
    root: Path,
    origin: str,
    content: str,
    options: dict[str, Any] | None = None,
    *,
    settings: dict[str, Any] | None = None,
) -> Path:
    docs = root / "docs"
    overrides = root / "overrides"
    docs.mkdir(exist_ok=True)
    overrides.mkdir(exist_ok=True)
    (docs / "index.md").write_text(content)
    (overrides / "main.html").write_text("""<!doctype html><html><head>
{% for path in config.extra_css %}
<link rel="stylesheet" href="{{ path | url }}">{% endfor %}
{% for script in config.extra_javascript %}
<script src="{{ script.path | url }}"></script>{% endfor %}
</head><body>{{ page.content }}</body></html>""")
    privacy = {
        "assets_include": [origin.removeprefix("http://") + "/*"],
        **(options or {}),
    }
    config: dict[str, Any] = {
        "site_name": "Privacy",
        "site_url": "https://example.com/docs/",
        "theme": {
            "name": "material",
            "custom_dir": "overrides",
            "font": False,
            "static_templates": [],
        },
        "plugins": [{"privacy": privacy}],
        **(settings or {}),
    }
    path = root / "mkdocs.yml"
    # JSON is also accepted as YAML, with no test-specific YAML serialization.
    path.write_text(json.dumps(config))
    return path


def _build(config: Path, *, strict: bool = False) -> None:
    zensical.build(str(config), {"clean": False, "strict": strict})


def _soup(root: Path, page: str = "index.html") -> BeautifulSoup:
    return BeautifulSoup((root / "site" / page).read_text(), "html.parser")


def _tag(soup: BeautifulSoup, selector: str) -> Tag:
    tag = soup.select_one(selector)
    assert isinstance(tag, Tag), f"Missing expected element: {selector}"
    return tag


def _attribute(soup: BeautifulSoup, selector: str, name: str) -> str:
    value = _tag(soup, selector)[name]
    assert isinstance(value, str)
    return value


def test_rewrites_html_assets_preserves_fragments_and_annotates_links(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/picture?size=2"] = ("image/svg+xml", b"<svg></svg>")
    responses["/app.js"] = ("text/javascript", b"window.answer = 42;")
    config = _project(
        tmp_path,
        origin,
        f"""
<link rel="preconnect" href="{origin}" crossorigin>
<link rel="icon" href="{origin}/picture?size=2">
<img src="{origin}/picture?size=2#only-light">
<svg><image href="{origin}/picture?size=2#only-dark" /></svg>
<script src="{origin}/app.js" async defer></script>
<a href="https://external.example/" target="_blank" rel="nofollow">External</a>
<a href="https://example.com/docs/" target="_blank">Same host</a>
<a href="local/">Local</a>
""",
        {"links_attr_map": {"data-external": "yes"}},
    )

    _build(config)

    soup = _soup(tmp_path)
    assert soup.find("link", rel="preconnect") is None
    assert _attribute(soup, "img", "src").endswith(".svg#only-light")
    assert _attribute(soup, "image", "href").endswith(".svg#only-dark")
    assert origin not in _attribute(soup, "img", "src")
    assert _tag(soup, "script").has_attr("async")
    assert _tag(soup, "script").has_attr("defer")
    external = _tag(soup, 'a[href="https://external.example/"]')
    assert external["rel"] == ["nofollow", "noopener"]
    assert (
        _attribute(soup, 'a[href="https://example.com/docs/"]', "data-external")
        == "yes"
    )
    assert not _tag(soup, 'a[href="local/"]').has_attr("data-external")
    assert requests == {"/picture?size=2": 1, "/app.js": 1}

    image = next((tmp_path / "site/assets/external").rglob("*.svg"))
    assert image.read_bytes() == b"<svg></svg>"
    assert hashlib.sha1(b"size=2").hexdigest()[:8] in image.name  # noqa: S324


def test_recursively_patches_remote_and_local_media_before_minify_hashing(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/fonts.css"] = (
        "text/css",
        f'@font-face {{ src: url("{origin}/font.woff2"); }}'.encode(),
    )
    responses["/font.woff2"] = ("font/woff2", b"font bytes")
    responses["/dependency.js"] = (
        "application/javascript",
        f'const data = "{origin}/data.json";'.encode(),
    )
    responses["/data.json"] = ("application/json", b'{"answer":42}')
    config = _project(
        tmp_path,
        origin,
        "# Media",
        settings={
            "extra_css": ["styles/site.css", origin + "/fonts.css"],
            "extra_javascript": ["assets/loader.js"],
            "plugins": [
                {
                    "privacy": {
                        "assets_include": [
                            origin.removeprefix("http://") + "/*"
                        ]
                    }
                },
                {
                    "minify": {
                        "minify_css": True,
                        "minify_js": True,
                        "cache_safe": True,
                        "css_files": ["styles/site.css"],
                        "js_files": ["assets/loader.js"],
                    }
                },
            ],
        },
    )
    (tmp_path / "docs/styles").mkdir()
    (tmp_path / "docs/assets").mkdir()
    (tmp_path / "docs/styles/site.css").write_text(
        f'@import url("{origin}/fonts.css");'
    )
    (tmp_path / "docs/assets/loader.js").write_text(
        f'const source = "{origin}/dependency.js";'
    )

    _build(config)

    assert requests == {
        "/fonts.css": 1,
        "/font.woff2": 1,
        "/dependency.js": 1,
        "/data.json": 1,
    }
    remote_css = next((tmp_path / "site/assets/external").rglob("fonts.css"))
    assert 'url("font.woff2")' in remote_css.read_text()
    assert origin not in remote_css.read_text()
    remote_js = next((tmp_path / "site/assets/external").rglob("dependency.js"))
    data = next((tmp_path / "site/assets/external").rglob("data.json"))
    data_url = "/docs/" + data.relative_to(tmp_path / "site").as_posix()
    assert f'"{data_url}"' in remote_js.read_text()
    loader = next((tmp_path / "site/assets").glob("loader.*.min.js"))
    assert "/docs/assets/external/" in loader.read_text()
    assert "https://example.com" not in loader.read_text()
    assert hashlib.sha384(loader.read_bytes()).hexdigest()[:6] in loader.name
    stylesheet = next((tmp_path / "site/styles").glob("site.*.min.css"))
    assert origin not in stylesheet.read_text()
    digest = hashlib.sha384(stylesheet.read_bytes()).hexdigest()[:6]
    assert digest in stylesheet.name


@pytest.mark.parametrize("source", ["local", "remote"])
@pytest.mark.parametrize(
    "site_url",
    [
        None,
        "https://example.com/",
        "https://example.com/docs/",
        "https://example.com/docs",
    ],
)
def test_javascript_dependencies_resolve_from_root_and_nested_pages(
    tmp_path: Path, remote: Any, source: str, site_url: str | None
) -> None:
    origin, responses, requests = remote
    loader = f"""
const script = document.createElement("script");
script.src = "{origin}/dependency.js";
document.head.appendChild(script);
const link = document.createElement("link");
link.rel = "stylesheet";
link.href = "{origin}/dependency.css";
document.head.appendChild(link);
"""
    responses["/dependency.js"] = ("text/javascript", b"window.loaded = true;")
    responses["/dependency.css"] = (
        "text/css",
        f'@font-face {{ src: url("{origin}/font.woff2"); }}'.encode(),
    )
    responses["/font.woff2"] = ("font/woff2", b"font bytes")
    if source == "remote":
        responses["/loader.js"] = ("text/javascript", loader.encode())
        loader_path = origin + "/loader.js"
    else:
        loader_path = "scripts/loader.js"

    config = _project(
        tmp_path,
        origin,
        "# Dynamic assets",
        settings={
            "site_url": site_url,
            "extra_javascript": [loader_path],
        },
    )
    if source == "local":
        (tmp_path / "docs/scripts").mkdir()
        (tmp_path / "docs/scripts/loader.js").write_text(loader)
    (tmp_path / "docs/guide").mkdir()
    (tmp_path / "docs/guide/page.md").write_text("# Nested dynamic assets")

    _build(config)

    site = tmp_path / "site"
    patched_loader = next(site.rglob("loader.js")).read_text()
    dependencies = re.findall(r'\.(?:src|href) = "([^"]+)"', patched_loader)
    assert len(dependencies) == 2

    # Dynamic DOM URLs are resolved against each page by the browser.
    # The deployment path is retained while the preview host is used.
    base_path = urlsplit(site_url or "/").path.strip("/")
    preview = "http://localhost:8000/" + (base_path + "/" if base_path else "")
    for page in ("", "guide/page/"):
        page_url = urljoin(preview, page)
        soup = _soup(tmp_path, page + "index.html")
        script_url = urljoin(page_url, _attribute(soup, "script", "src"))
        assert script_url.startswith(preview)
        assert (site / script_url.removeprefix(preview)).is_file()

        for dependency in dependencies:
            resolved = urljoin(page_url, dependency)
            assert resolved.startswith(preview)
            assert (site / resolved.removeprefix(preview)).is_file()

    # Font URLs are still resolved relative to the downloaded stylesheet.
    stylesheet = next(site.rglob("dependency.css"))
    assert 'url("font.woff2")' in stylesheet.read_text()
    assert (stylesheet.parent / "font.woff2").is_file()
    assert requests == {
        **({"/loader.js": 1} if source == "remote" else {}),
        "/dependency.js": 1,
        "/dependency.css": 1,
        "/font.woff2": 1,
    }


def test_raw_download_cache_is_reused_and_outputs_restored_after_cleaning(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/image.png"] = ("image/png", b"cached image")
    config = _project(tmp_path, origin, f"![Image]({origin}/image.png)")

    _build(config)
    downloaded = next((tmp_path / "site/assets/external").rglob("image.png"))
    downloaded.unlink()
    responses.clear()
    _build(config)

    assert downloaded.read_bytes() == b"cached image"
    assert requests["/image.png"] == 1


@pytest.mark.parametrize("option", ["enabled", "assets"])
def test_disabled_fetching_preserves_urls_without_requests(
    tmp_path: Path, remote: Any, option: str
) -> None:
    origin, _responses, requests = remote
    config = _project(
        tmp_path, origin, f"![Image]({origin}/image.png)", {option: False}
    )

    _build(config)

    assert _attribute(_soup(tmp_path), "img", "src") == origin + "/image.png"
    assert not requests


def test_report_only_warns_and_fails_in_strict_mode(
    tmp_path: Path, remote: Any, capfd: pytest.CaptureFixture[str]
) -> None:
    origin, _responses, requests = remote
    config = _project(
        tmp_path,
        origin,
        f"![Image]({origin}/image.png)",
        {"assets_fetch": False, "assets_include": []},
    )

    _build(config)

    assert "External file:" in capfd.readouterr().err
    assert _attribute(_soup(tmp_path), "img", "src") == origin + "/image.png"
    assert not requests

    with pytest.raises(RuntimeError, match="strict"):
        _build(config, strict=True)


def test_failed_download_keeps_original_url_and_reports_warning(
    tmp_path: Path, remote: Any, capfd: pytest.CaptureFixture[str]
) -> None:
    origin, _responses, requests = remote
    config = _project(tmp_path, origin, f"![Missing]({origin}/missing.png)")

    _build(config)

    assert _attribute(_soup(tmp_path), "img", "src") == origin + "/missing.png"
    assert "Couldn't retrieve" in capfd.readouterr().err
    assert requests["/missing.png"] == 1


@pytest.mark.parametrize("status", [404, 500])
def test_nested_pages_and_error_templates_use_their_own_url_bases(
    tmp_path: Path, remote: Any, status: int
) -> None:
    origin, responses, requests = remote
    responses["/image.png"] = ("image/png", b"image")
    content = f'<img src="{origin}/image.png">'
    config = _project(tmp_path, origin, content)
    guide = tmp_path / "docs/guide"
    guide.mkdir()
    (guide / "page.md").write_text(content)
    (tmp_path / "overrides/error.html").write_text(content)
    (tmp_path / "docs/error.html").write_text(content)
    options = json.loads(config.read_text())
    options["theme"]["static_templates"] = ["error.html"]
    config.write_text(json.dumps(options))

    # The HTTP error URL base and an extra template are exercised together.
    (tmp_path / "docs" / f"{status}.html").write_text(content)
    (tmp_path / "overrides" / f"{status}.html").write_text(content)
    options["extra_templates"] = [f"{status}.html", "error.html"]
    config.write_text(json.dumps(options))

    _build(config)

    nested = _attribute(_soup(tmp_path, "guide/page/index.html"), "img", "src")
    error = _attribute(_soup(tmp_path, f"{status}.html"), "img", "src")
    extra = _attribute(_soup(tmp_path, "error.html"), "img", "src")
    assert nested.startswith("../../assets/external/")
    assert error.startswith("/docs/assets/external/")
    assert extra.startswith("assets/external/")
    assert requests["/image.png"] == 1


def test_custom_python_regex_patches_unclaimed_resources_and_cycles(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/cycle.txt"] = (
        "text/plain",
        f'é: "{origin}/cycle.txt"'.encode(),
    )
    config = _project(
        tmp_path,
        origin,
        "# Custom regex",
        {
            "assets_expr_map": {
                ".txt": r'(?<=: )(["\'])(?P<url>https?://[^"\']+)\1'
            },
        },
    )
    (tmp_path / "docs/local.txt").write_text(f'é: "{origin}/cycle.txt"')

    _build(config)

    local = (tmp_path / "site/local.txt").read_text()
    assert local.startswith('é: "assets/external/')
    cycle = next((tmp_path / "site/assets/external").rglob("cycle.txt"))
    assert cycle.read_text() == 'é: "cycle.txt"'
    assert requests["/cycle.txt"] == 1


def test_disabled_cache_refreshes_downloads_between_builds(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/image.png"] = ("image/png", b"first")
    config = _project(
        tmp_path, origin, f"![Image]({origin}/image.png)", {"cache": False}
    )

    _build(config)
    responses["/image.png"] = ("image/png", b"second")
    _build(config)

    image = next((tmp_path / "site/assets/external").rglob("image.png"))
    assert image.read_bytes() == b"second"
    assert requests["/image.png"] == 2


def test_include_patterns_take_precedence_like_material(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/image.png"] = ("image/png", b"included")
    config = _project(
        tmp_path,
        origin,
        f"![Image]({origin}/image.png)",
        {
            "assets_exclude": ["*"],
            "assets_fetch": False,
        },
    )

    # Material returns an inclusion match before evaluating either option.
    _build(config)

    assert origin not in _attribute(_soup(tmp_path), "img", "src")
    assert requests["/image.png"] == 1


def test_named_instances_keep_distinct_asset_settings_and_link_order(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/image.png"] = ("image/png", b"named instance")
    config = _project(
        tmp_path,
        origin,
        f"![Image]({origin}/image.png)\n\n[External]({origin}/page)",
        settings={
            "plugins": [
                {
                    "material/privacy/images": {
                        "assets_include": [
                            origin.removeprefix("http://") + "/*"
                        ],
                        "assets_fetch_dir": "downloads",
                        "links_attr_map": {
                            "target": "_blank",
                            "data-order": "first",
                        },
                    }
                },
                {
                    "privacy/links": {
                        "assets": False,
                        "links_attr_map": {"data-order": "last"},
                    }
                },
            ]
        },
    )

    _build(config)

    soup = _soup(tmp_path)
    assert _attribute(soup, "img", "src").startswith("downloads/")
    assert _attribute(soup, "a", "data-order") == "last"
    assert _tag(soup, "a")["rel"] == ["noopener"]
    assert requests["/image.png"] == 1


@pytest.mark.parametrize("options", [{"log": False}, {"log_level": "error"}])
def test_filtered_warnings_do_not_fail_strict_builds(
    tmp_path: Path,
    remote: Any,
    options: dict[str, Any],
    capfd: pytest.CaptureFixture[str],
) -> None:
    origin, _responses, _requests = remote
    config = _project(
        tmp_path, origin, f"![Image]({origin}/missing.png)", options
    )

    _build(config, strict=True)

    assert "Couldn't retrieve" not in capfd.readouterr().err
    assert _attribute(_soup(tmp_path), "img", "src") == origin + "/missing.png"


def test_same_hostname_assets_are_preserved_and_links_can_be_disabled(
    tmp_path: Path, remote: Any
) -> None:
    origin, _responses, requests = remote
    config = _project(
        tmp_path,
        origin,
        f'<img src="{origin}/image.png">'
        '<a href="https://external.example" target="_blank">Link</a>',
        {"links": False},
        settings={"site_url": origin + "/docs/"},
    )

    _build(config)

    soup = _soup(tmp_path)
    assert _attribute(soup, "img", "src") == origin + "/image.png"
    assert not _tag(soup, "a").has_attr("rel")
    assert not requests


def test_extensionless_font_stylesheets_and_protocol_relative_urls(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/css?family=Font&display=swap"] = (
        "text/css; charset=utf-8",
        f"@font-face {{ src: url({origin}/font.woff2); }}".encode(),
    )
    responses["/font.woff2"] = ("font/woff2", b"font")
    protocol_relative = origin.removeprefix("http:")
    config = _project(
        tmp_path,
        origin,
        f"""
<link rel="preconnect" href="{origin}">
<link rel="stylesheet"
 href="{protocol_relative}/css?family=Font&amp;display=swap">
""",
    )

    _build(config)

    soup = _soup(tmp_path)
    assert soup.find("link", rel="preconnect") is None
    assert _attribute(soup, "link[rel=stylesheet]", "href").endswith(".css")
    stylesheet = next((tmp_path / "site/assets/external").rglob("*.css"))
    assert "url(font.woff2)" in stylesheet.read_text()
    assert requests == {"/css?family=Font&display=swap": 1, "/font.woff2": 1}


def test_mermaid_is_preloaded_without_a_site_url_for_nested_pages(
    tmp_path: Path, remote: Any
) -> None:
    origin, responses, requests = remote
    responses["/mermaid.min.js"] = ("text/javascript", b"window.mermaid = {};")
    config = _project(
        tmp_path,
        origin,
        "# Mermaid",
        settings={
            "site_url": None,
            "extra_javascript": ["loader.js"],
        },
    )
    (tmp_path / "docs/loader.js").write_text(
        f'const mermaid = "{origin}/mermaid.min.js";'
    )
    guide = tmp_path / "docs/guide"
    guide.mkdir()
    (guide / "page.md").write_text("# Nested Mermaid")

    _build(config)

    nested = _soup(tmp_path, "guide/page/index.html")
    preload = _attribute(nested, "script[src*=mermaid]", "src")
    assert preload.startswith("../../assets/external/")
    assert preload.endswith("mermaid.min.js")
    assert origin not in (tmp_path / "site/loader.js").read_text()
    assert requests["/mermaid.min.js"] == 1
