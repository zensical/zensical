// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Native JPEG encoding and pngquant process handling.

use anyhow::{anyhow, bail, Context as _, Result};
use jpeg_encoder::{ColorType, Encoder};
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::thread;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// JPEG pixels are decoded and encoded with the configured quality and scans.
pub(super) fn jpeg(
    bytes: &[u8], quality: u8, progressive: bool,
) -> Result<Vec<u8>> {
    let options = DecoderOptions::default()
        .set_max_width(usize::from(u16::MAX))
        .set_max_height(usize::from(u16::MAX));
    let mut decoder =
        JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .context("failed to read JPEG headers")?;
    let grayscale = decoder.input_colorspace() == Some(ColorSpace::Luma);
    let colorspace = if grayscale {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    };
    decoder.set_options(options.jpeg_set_out_colorspace(colorspace));
    let pixels = decoder.decode().context("failed to decode JPEG")?;
    let (width, height) =
        decoder.dimensions().context("missing JPEG dimensions")?;
    let mut output = Vec::new();
    // Material accepts quality 0. The same minimum quantization quality is
    // selected by libjpeg and jpeg-encoder when the quality is below 1.
    let mut encoder = Encoder::new(&mut output, quality.max(1));
    encoder.set_progressive(progressive);
    encoder.set_optimized_huffman_tables(true);
    encoder
        .encode(
            &pixels,
            u16::try_from(width)?,
            u16::try_from(height)?,
            if grayscale {
                ColorType::Luma
            } else {
                ColorType::Rgb
            },
        )
        .context("failed to encode JPEG")?;
    Ok(output)
}

/// PNG bytes are quantized using the user-installed pngquant executable.
pub(super) fn png(bytes: &[u8], speed: u8, strip: bool) -> Result<Vec<u8>> {
    let mut command = Command::new("pngquant");
    command.args(["--speed", &speed.to_string(), "--skip-if-larger"]);
    if strip {
        command.arg("--strip");
    }
    let mut child = command
        .args(["--output", "-", "--", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                anyhow!("PNG optimization requires pngquant to be installed and available on PATH")
            } else {
                anyhow!(error).context("failed to start pngquant")
            }
        })?;
    let mut stdin = child.stdin.take().context("missing pngquant stdin")?;
    // Input is written while stdout and stderr are drained. Large PNGs could
    // otherwise fill both pipes and deadlock the build.
    let (written, output) = thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(bytes));
        let output = child.wait_with_output();
        (writer.join(), output)
    });
    let output = output.context("failed to wait for pngquant")?;
    match output.status.code() {
        // Size and quality rejections are treated as unchanged images, even
        // when pngquant exits before all input bytes have been consumed.
        Some(98 | 99) => Ok(bytes.to_vec()),
        Some(0) => {
            written
                .map_err(|_| anyhow!("pngquant input writer panicked"))?
                .context("failed to write PNG to pngquant")?;
            if output.stdout.is_empty() {
                bail!("pngquant returned an empty image");
            }
            Ok(output.stdout)
        }
        _ => bail!(
            "pngquant failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHOTO: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../python/tests/fixtures/optimize/photo.jpg"
    ));
    const GRAYSCALE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../python/tests/fixtures/optimize/grayscale.jpg"
    ));
    const CMYK: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../python/tests/fixtures/optimize/cmyk.jpg"
    ));

    #[test]
    fn jpeg_retains_dimensions_and_configured_scan_mode() {
        for progressive in [false, true] {
            let output = jpeg(PHOTO, 60, progressive).unwrap();
            let mut decoder = JpegDecoder::new(ZCursor::new(&output));

            let pixels = decoder.decode().unwrap();

            assert_eq!(decoder.dimensions(), Some((128, 96)));
            assert_eq!(
                decoder.info().unwrap().sof.is_progressive(),
                progressive
            );
            assert_eq!(pixels.len(), 128 * 96 * 3);
            assert!(output.len() < PHOTO.len());
        }
    }

    #[test]
    fn jpeg_quality_zero_uses_minimum_quality() {
        let minimum = jpeg(PHOTO, 0, true).unwrap();
        let one = jpeg(PHOTO, 1, true).unwrap();
        let maximum = jpeg(PHOTO, 100, true).unwrap();

        assert_eq!(minimum, one);
        assert!(minimum.len() < maximum.len());
    }

    #[test]
    fn jpeg_keeps_grayscale_components() {
        let output = jpeg(GRAYSCALE, 60, true).unwrap();
        let mut decoder = JpegDecoder::new(ZCursor::new(&output));

        decoder.decode().unwrap();

        assert_eq!(decoder.dimensions(), Some((128, 96)));
        assert_eq!(decoder.info().unwrap().components, 1);
    }

    #[test]
    fn jpeg_converts_cmyk_without_inverting_colors() {
        // A red CMYK fixture is used to expose inverted or swapped channels.
        let output = jpeg(CMYK, 60, true).unwrap();
        let options =
            DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
        let mut decoder =
            JpegDecoder::new_with_options(ZCursor::new(&output), options);

        let pixels = decoder.decode().unwrap();

        assert_eq!(decoder.dimensions(), Some((128, 96)));
        for pixel in pixels.as_chunks::<3>().0 {
            assert!(pixel[0] > 240, "{pixel:?}");
            assert!(pixel[1] < 15, "{pixel:?}");
            assert!(pixel[2] < 15, "{pixel:?}");
        }
    }

    #[test]
    fn progressive_jpeg_can_be_reencoded_as_baseline() {
        let progressive = jpeg(PHOTO, 85, true).unwrap();

        let baseline = jpeg(&progressive, 60, false).unwrap();
        let mut decoder = JpegDecoder::new(ZCursor::new(&baseline));
        decoder.decode().unwrap();

        assert!(!decoder.info().unwrap().sof.is_progressive());
    }

    #[test]
    fn invalid_jpeg_is_reported() {
        let error = jpeg(b"not a JPEG", 60, true).unwrap_err();

        assert!(error.to_string().contains("failed to read JPEG headers"));
    }
}
