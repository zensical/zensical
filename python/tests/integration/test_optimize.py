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

"""Build-level tests for Material-compatible image optimization."""

from __future__ import annotations

import json
import shutil
import struct
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import pytest

import zensical

_FIXTURES = Path(__file__).parents[1] / "fixtures" / "optimize"


def _project(
    root: Path, options: dict[str, Any] | None = None, *, toml: bool = False
) -> Path:
    """Create a small project with images selected independently of Markdown."""
    (root / "docs" / "images").mkdir(parents=True)
    (root / "docs" / "index.md").write_text(
        "# Images\n\n![Photo](images/photo.jpg)\n"
    )
    (root / "overrides").mkdir()
    (root / "overrides" / "main.html").write_text("{{ page.content }}")
    options = {"optimize_include": ["images/*"], **(options or {})}
    if toml:
        path = root / "zensical.toml"
        path.write_text(
            '[project]\nsite_name = "Optimize"\n[project.theme]\n'
            'custom_dir = "overrides"\nstatic_templates = []\n'
            "font = false\n[project.plugins.optimize]\n"
            + "\n".join(
                f"{key} = {json.dumps(value)}" for key, value in options.items()
            )
        )
    else:
        path = root / "mkdocs.yml"
        path.write_text(
            json.dumps(
                {
                    "site_name": "Optimize",
                    "theme": {
                        "custom_dir": "overrides",
                        "static_templates": [],
                        "font": False,
                    },
                    "plugins": [{"material/optimize": options}],
                }
            )
        )
    return path


def _photo(path: Path, fixture: str = "photo.jpg") -> bytes:
    """Copy a deterministic JPEG fixture into a test project."""
    data = (_FIXTURES / fixture).read_bytes()
    path.write_bytes(data)
    return data


def _jpeg_frame(path: Path) -> tuple[int, int, bool]:
    """Read dimensions and progressive mode from the JPEG frame header."""
    data = path.read_bytes()
    assert data.startswith(b"\xff\xd8")
    offset = 2
    while offset + 4 <= len(data):
        assert data[offset] == 0xFF
        marker = data[offset + 1]
        length = int.from_bytes(data[offset + 2 : offset + 4], "big")
        if marker in (0xC0, 0xC1, 0xC2):
            height, width = struct.unpack(">HH", data[offset + 5 : offset + 9])
            return width, height, marker == 0xC2
        offset += 2 + length
    raise AssertionError("JPEG contains no supported frame header")


def _png_chunks(path: Path) -> dict[bytes, bytes]:
    """Read PNG chunks for dimensions, metadata, and palette transparency."""
    data = path.read_bytes()
    assert data.startswith(b"\x89PNG\r\n\x1a\n")
    chunks = {}
    offset = 8
    while offset < len(data):
        length = int.from_bytes(data[offset : offset + 4], "big")
        kind = data[offset + 4 : offset + 8]
        chunks[kind] = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
    return chunks


def _build(config: Path) -> None:
    zensical.build(str(config), {"clean": False, "strict": False})


@pytest.mark.parametrize("toml", [False, True])
def test_build_optimizes_unreferenced_images_and_keeps_urls(
    tmp_path: Path, toml: bool
) -> None:
    config = _project(tmp_path, toml=toml)
    original = _photo(tmp_path / "docs" / "images" / "photo.jpg")
    _photo(tmp_path / "docs" / "images" / "unreferenced.JPEG")
    (tmp_path / "docs" / "images" / "unchanged.txt").write_bytes(b"unchanged")

    _build(config)

    output = tmp_path / "site" / "images" / "photo.jpg"
    assert len(output.read_bytes()) < len(original)
    assert (tmp_path / "docs" / "images" / "photo.jpg").read_bytes() == original
    assert (
        tmp_path / "site" / "images" / "unchanged.txt"
    ).read_bytes() == b"unchanged"
    assert "images/photo.jpg" in (tmp_path / "site" / "index.html").read_text()
    assert _jpeg_frame(output) == (128, 96, True)
    assert _jpeg_frame(tmp_path / "site" / "images" / "unreferenced.JPEG") == (
        128,
        96,
        True,
    )


@pytest.mark.skipif(
    shutil.which("pngquant") is None, reason="pngquant is required"
)
@pytest.mark.parametrize("strip", [False, True])
def test_png_optimization_respects_metadata_and_transparency(
    tmp_path: Path, strip: bool
) -> None:
    config = _project(
        tmp_path, {"optimize_png_speed": 10, "optimize_png_strip": strip}
    )
    source = tmp_path / "docs" / "images" / "diagram.png"
    source.write_bytes((_FIXTURES / "diagram.png").read_bytes())

    _build(config)

    output = tmp_path / "site" / "images" / "diagram.png"
    assert len(output.read_bytes()) <= len(source.read_bytes())
    data = output.read_bytes()
    chunks = _png_chunks(output)
    assert struct.unpack(">II", chunks[b"IHDR"][:8]) == (64, 64)
    assert (b"Description" not in data) is strip
    if data != source.read_bytes():
        # The single red palette entry must retain alpha 128 after quantization.
        assert chunks[b"PLTE"] == b"\xff\x00\x00"
        assert chunks[b"tRNS"] == b"\x80"


@pytest.mark.parametrize(
    "options",
    [
        {"enabled": False},
        {"optimize": False},
        {"optimize_jpg": False},
        {
            "optimize_exclude": ["images/*"],
            "optimize_include": [],
            "optimize_png": False,
        },
    ],
)
def test_disabled_or_excluded_images_are_copied_verbatim(
    tmp_path: Path, options: dict[str, Any]
) -> None:
    config = _project(tmp_path, options)
    # Invalid bytes are used so any accidental encoding would fail the build.
    (tmp_path / "docs" / "images" / "photo.jpg").write_bytes(b"not an image")

    _build(config)

    assert (
        tmp_path / "site" / "images" / "photo.jpg"
    ).read_bytes() == b"not an image"
    assert not (tmp_path / ".cache" / "plugin" / "optimize").exists()


def test_include_patterns_take_precedence_over_exclusions(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path, {"optimize_exclude": ["*"]})
    original = _photo(tmp_path / "docs" / "images" / "photo.jpg")

    _build(config)

    assert len((tmp_path / "site" / "images" / "photo.jpg").read_bytes()) < len(
        original
    )


def test_cache_reuses_bytes_and_invalidates_for_sources_and_options(
    tmp_path: Path,
) -> None:
    config = _project(tmp_path)
    source = tmp_path / "docs" / "images" / "photo.jpg"
    _photo(source)
    _build(config)
    first = (tmp_path / "site" / "images" / "photo.jpg").read_bytes()
    cache = tmp_path / ".cache" / "plugin" / "optimize"
    cached = list(cache.iterdir())
    assert len(cached) == 1
    modified = cached[0].stat().st_mtime_ns

    _build(config)

    assert (tmp_path / "site" / "images" / "photo.jpg").read_bytes() == first
    assert cached[0].stat().st_mtime_ns == modified
    assert list(cache.iterdir()) == cached

    # New entries must be created when source bytes or encoder settings change.
    _photo(source, "alternate.jpg")
    _build(config)

    assert (tmp_path / "site" / "images" / "photo.jpg").read_bytes() != first
    assert len(list(cache.iterdir())) == 2

    settings = json.loads(config.read_text())
    settings["plugins"][0]["material/optimize"]["optimize_jpg_progressive"] = (
        False
    )
    config.write_text(json.dumps(settings))
    _build(config)

    assert _jpeg_frame(tmp_path / "site" / "images" / "photo.jpg") == (
        128,
        96,
        False,
    )
    assert len(list(cache.iterdir())) == 3


def test_cache_can_be_disabled(tmp_path: Path) -> None:
    config = _project(tmp_path, {"cache": False})
    original = _photo(tmp_path / "docs" / "images" / "photo.jpg")

    _build(config)
    _build(config)

    assert len((tmp_path / "site" / "images" / "photo.jpg").read_bytes()) < len(
        original
    )
    assert not (tmp_path / ".cache" / "plugin" / "optimize").exists()


def test_corrupt_image_reports_its_path(tmp_path: Path) -> None:
    config = _project(tmp_path)
    (tmp_path / "docs" / "images" / "photo.jpg").write_bytes(b"invalid JPEG")

    with pytest.raises(
        RuntimeError, match=r"failed to optimize images/photo\.jpg"
    ):
        _build(config)


def test_gain_reporting_can_be_disabled(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    config = _project(tmp_path)
    _photo(tmp_path / "docs" / "images" / "photo.jpg")

    _build(config)

    report = capfd.readouterr().err
    assert "photo.jpg: saved" in report
    assert "Total savings:" in report

    settings = json.loads(config.read_text())
    settings["plugins"][0]["material/optimize"].update(
        print_gain=False, print_gain_summary=False
    )
    config.write_text(json.dumps(settings))

    _build(config)

    assert "[optimize]" not in capfd.readouterr().err


def test_serve_copies_images_without_encoding(tmp_path: Path) -> None:
    config = _project(tmp_path)
    (tmp_path / "docs" / "images" / "photo.jpg").write_bytes(b"invalid JPEG")
    log = tmp_path / "serve.log"
    with log.open("w") as stream:
        process = subprocess.Popen(  # noqa: S603
            [
                sys.executable,
                "-m",
                "zensical",
                "serve",
                "--config-file",
                str(config),
                "--dev-addr",
                "127.0.0.1:0",
            ],
            stdout=stream,
            stderr=stream,
        )
    try:
        output = tmp_path / "site" / "images" / "photo.jpg"
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            assert process.poll() is None, log.read_text()
            if output.exists() and output.read_bytes() == b"invalid JPEG":
                break
            time.sleep(0.05)
        else:
            pytest.fail(log.read_text())
    finally:
        process.terminate()
        process.wait(timeout=10)


def test_first_matching_instance_claims_each_image(tmp_path: Path) -> None:
    config = _project(tmp_path)
    _photo(tmp_path / "docs" / "images" / "photo.jpg")
    _photo(tmp_path / "docs" / "images" / "other.jpg")
    settings = json.loads(config.read_text())
    settings["plugins"] = [
        {
            "material/optimize/photo": {
                "optimize_include": ["images/photo.jpg"],
                "optimize_jpg_progressive": False,
            }
        },
        {
            "optimize/rest": {
                "optimize_include": ["images/*"],
                "optimize_jpg_progressive": True,
            }
        },
    ]
    config.write_text(json.dumps(settings))

    _build(config)

    assert _jpeg_frame(tmp_path / "site" / "images" / "photo.jpg") == (
        128,
        96,
        False,
    )
    assert _jpeg_frame(tmp_path / "site" / "images" / "other.jpg") == (
        128,
        96,
        True,
    )


@pytest.mark.parametrize("absolute", [False, True])
def test_custom_cache_directory_is_resolved_from_project(
    tmp_path: Path, absolute: bool
) -> None:
    cache = tmp_path / "image-cache"
    config = _project(
        tmp_path, {"cache_dir": str(cache) if absolute else "image-cache"}
    )
    _photo(tmp_path / "docs" / "images" / "photo.jpg")

    _build(config)

    assert list(cache.iterdir())
    assert not (tmp_path / ".cache" / "plugin" / "optimize").exists()


def test_extensionless_files_are_not_treated_as_images(tmp_path: Path) -> None:
    config = _project(tmp_path, {"optimize_include": ["*"]})
    for name in ("jpg", "jpeg", "png"):
        (tmp_path / "docs" / name).write_bytes(b"extensionless file")

    _build(config)

    for name in ("jpg", "jpeg", "png"):
        assert (tmp_path / "site" / name).read_bytes() == b"extensionless file"


def _fake_pngquant(
    root: Path, monkeypatch: pytest.MonkeyPatch, script: str
) -> Path:
    """Install a controlled pngquant for subprocess behavior tests."""
    if sys.platform == "win32":
        pytest.skip("The executable test fixture requires a POSIX shebang")
    binaries = root / "bin"
    binaries.mkdir()
    executable = binaries / "pngquant"
    executable.write_text(f"#!{sys.executable}\nimport sys\n{script}\n")
    executable.chmod(0o755)
    monkeypatch.setenv("PATH", str(binaries))
    return executable


@pytest.mark.parametrize("returncode", [98, 99])
def test_pngquant_rejections_keep_original_bytes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, returncode: int
) -> None:
    config = _project(tmp_path)
    original = (_FIXTURES / "diagram.png").read_bytes()
    (tmp_path / "docs" / "images" / "diagram.png").write_bytes(original)
    _fake_pngquant(tmp_path, monkeypatch, f"sys.exit({returncode})")

    _build(config)

    assert (
        tmp_path / "site" / "images" / "diagram.png"
    ).read_bytes() == original


def test_larger_pngquant_results_are_discarded(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _project(tmp_path)
    original = (_FIXTURES / "diagram.png").read_bytes()
    (tmp_path / "docs" / "images" / "diagram.png").write_bytes(original)
    _fake_pngquant(
        tmp_path,
        monkeypatch,
        "data = sys.stdin.buffer.read()\n"
        'sys.stdout.buffer.write(data + b"larger")',
    )

    _build(config)

    assert (
        tmp_path / "site" / "images" / "diagram.png"
    ).read_bytes() == original


def test_pngquant_error_includes_backend_diagnostic(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _project(tmp_path)
    (tmp_path / "docs" / "images" / "diagram.png").write_bytes(b"invalid PNG")
    _fake_pngquant(
        tmp_path, monkeypatch, 'sys.stderr.write("invalid image")\nsys.exit(1)'
    )

    with pytest.raises(RuntimeError, match=r"pngquant failed .*invalid image"):
        _build(config)


def test_missing_pngquant_explains_required_dependency(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _project(tmp_path)
    (tmp_path / "docs" / "images" / "diagram.png").write_bytes(b"PNG")
    binaries = tmp_path / "empty-bin"
    binaries.mkdir()
    monkeypatch.setenv("PATH", str(binaries))

    with pytest.raises(RuntimeError, match="requires pngquant"):
        _build(config)


def test_warm_png_cache_does_not_require_pngquant(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _project(tmp_path)
    source = tmp_path / "docs" / "images" / "diagram.png"
    original = (_FIXTURES / "diagram.png").read_bytes()
    source.write_bytes(original)
    executable = _fake_pngquant(
        tmp_path,
        monkeypatch,
        "sys.stdout.buffer.write(sys.stdin.buffer.read())",
    )
    _build(config)
    executable.unlink()

    _build(config)

    assert (
        tmp_path / "site" / "images" / "diagram.png"
    ).read_bytes() == original

    # Changed sources must require a new encoder invocation.
    source.write_bytes(original + b"changed")

    with pytest.raises(RuntimeError, match="requires pngquant"):
        _build(config)


def test_large_pngquant_pipes_do_not_deadlock(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _project(tmp_path)
    original = (_FIXTURES / "diagram.png").read_bytes() + b"x" * 1_000_000
    (tmp_path / "docs" / "images" / "diagram.png").write_bytes(original)
    # Large stdout and stderr buffers are emitted before stdin is consumed.
    _fake_pngquant(
        tmp_path,
        monkeypatch,
        'sys.stdout.buffer.write(b"x" * 1_000_000)\nsys.stdout.buffer.flush()\n'
        'sys.stderr.write("x" * 1_000_000)\nsys.stderr.flush()\n'
        "sys.stdout.buffer.write(sys.stdin.buffer.read())",
    )

    result = subprocess.run(  # noqa: S603
        [
            sys.executable,
            "-m",
            "zensical",
            "build",
            "--config-file",
            str(config),
        ],
        capture_output=True,
        timeout=20,
        check=False,
    )

    assert result.returncode == 0, result.stderr.decode()
    assert (
        tmp_path / "site" / "images" / "diagram.png"
    ).read_bytes() == original
