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

//! Deterministic Google Font acquisition and loading.

use anyhow::{anyhow, bail, Context, Result};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use regex::Regex;
use resvg::usvg::fontdb::{Database, Family, Query, Stretch, Style, Weight};
use sha2::{Digest, Sha256};
use skrifa::{string::StringId, MetadataProvider};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use super::layout::Font;
use super::plugin_error;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

static DOWNLOADS: Mutex<()> = Mutex::new(());
static FONT_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\"(https:[^\"]+\.[ot]tf)\""#)
        .expect("constant regular expression")
});

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Shared font resolver for one social plugin instance.
#[derive(Clone, Debug)]
pub struct Fonts {
    /// Directory holding downloaded font faces.
    cache: PathBuf,
    /// HTTP client for Google Fonts requests.
    agent: ureq::Agent,
    /// Selected font faces shared across card renders.
    loaded: Arc<Mutex<HashMap<Font, Arc<Database>>>>,
}

/// Normalized CSS font properties used by SVG and font database selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attributes {
    /// Numeric CSS font weight.
    pub weight: u16,
    /// CSS font style name.
    pub style: &'static str,
    /// CSS font stretch name.
    pub stretch: &'static str,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Fonts {
    /// Creates a resolver rooted in one instance cache directory.
    pub fn new(cache: PathBuf) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Self {
            cache: cache.join("fonts"),
            agent: config.into(),
            loaded: Arc::default(),
        }
    }

    /// Loads the closest face for a font request, downloading its family once.
    pub fn load(&self, font: &Font) -> Result<Arc<Database>> {
        if let Some(database) = self
            .loaded
            .lock()
            .map_err(|_| anyhow::anyhow!("social font cache was poisoned"))?
            .get(font)
            .cloned()
        {
            return Ok(database);
        }
        let _guard = DOWNLOADS.lock().map_err(|_| {
            anyhow::anyhow!("social font cache lock was poisoned")
        })?;
        if let Some(database) = self
            .loaded
            .lock()
            .map_err(|_| anyhow::anyhow!("social font cache was poisoned"))?
            .get(font)
            .cloned()
        {
            return Ok(database);
        }

        let directory = self.cache.join(safe_family(&font.family)?);
        let mut data = read_fonts(&directory)?;
        if data.is_empty() {
            self.fetch(&font.family, &directory)?;
            data = read_fonts(&directory)?;
        }
        if data.is_empty() {
            bail!(
                "Google Fonts returned no usable faces for '{}'",
                font.family
            )
        }
        let database = Arc::new(select_face(font, data)?);
        self.loaded
            .lock()
            .map_err(|_| anyhow::anyhow!("social font cache was poisoned"))?
            .insert(font.clone(), database.clone());
        Ok(database)
    }

    /// Downloads and validates the configured font family.
    fn fetch(&self, family: &str, directory: &Path) -> Result<()> {
        let encoded = utf8_percent_encode(family, NON_ALPHANUMERIC);
        let url =
            format!("https://fonts.google.com/download/list?family={encoded}");
        let mut response = match self.agent.get(&url).call() {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(status)) => {
                return Err(plugin_error(anyhow!(
                    "couldn't find font family '{family}' on Google Fonts \
                     ({status})"
                )));
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to fetch font family '{family}'")
                });
            }
        };
        let manifest = response
            .body_mut()
            .with_config()
            .limit(2 * 1024 * 1024)
            .read_to_string()
            .context("failed to read Google Fonts manifest")?;
        let urls = FONT_URL
            .captures_iter(&manifest)
            .filter_map(|captures| captures.get(1).map(|value| value.as_str()))
            .collect::<Vec<_>>();
        if urls.is_empty() {
            bail!("Google Fonts returned no downloadable faces for '{family}'")
        }
        fs::create_dir_all(directory)?;
        for url in urls {
            if !url.starts_with("https://fonts.gstatic.com/") {
                bail!("Google Fonts returned an unexpected font URL")
            }
            let mut response =
                self.agent.get(url).call().with_context(|| {
                    format!("failed to download font from {url}")
                })?;
            let data = response
                .body_mut()
                .with_config()
                .limit(32 * 1024 * 1024)
                .read_to_vec()
                .with_context(|| format!("failed to read font from {url}"))?;
            validate_font(&data).with_context(|| {
                format!("invalid font downloaded from {url}")
            })?;
            let digest = format!("{:x}", Sha256::digest(&data));
            let extension = url.rsplit('.').next().unwrap_or("ttf");
            let target = directory.join(format!("{digest}.{extension}"));
            let temporary = directory.join(format!(".{digest}.tmp"));
            fs::write(&temporary, data)?;
            fs::rename(temporary, target)?;
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns normalized properties for a configured font face.
pub fn attributes(font: &Font) -> Attributes {
    let style = font.style.to_ascii_lowercase();
    let compact = style.replace([' ', '-'], "");
    let weight = if compact.contains("thin") {
        100
    } else if compact.contains("extralight") {
        200
    } else if compact.contains("light") {
        300
    } else if compact.contains("medium") {
        500
    } else if compact.contains("semibold") {
        600
    } else if compact.contains("extrabold") {
        800
    } else if compact.contains("black") {
        900
    } else if compact.contains("bold") {
        700
    } else {
        400
    };
    let style = if style.contains("italic") {
        "italic"
    } else if style.contains("oblique") {
        "oblique"
    } else {
        "normal"
    };
    let variant = font.variant.to_ascii_lowercase().replace('-', " ");
    let stretch = if variant.contains("ultra condensed") {
        "ultra-condensed"
    } else if variant.contains("extra condensed") {
        "extra-condensed"
    } else if variant.contains("semi condensed") {
        "semi-condensed"
    } else if variant.contains("condensed") {
        "condensed"
    } else if variant.contains("ultra expanded") {
        "ultra-expanded"
    } else if variant.contains("extra expanded") {
        "extra-expanded"
    } else if variant.contains("semi expanded") {
        "semi-expanded"
    } else if variant.contains("expanded") {
        "expanded"
    } else {
        "normal"
    };
    Attributes { weight, style, stretch }
}

fn select_face(font: &Font, data: Vec<Vec<u8>>) -> Result<Database> {
    let mut database = Database::new();
    for data in data {
        database.load_font_data(data);
    }
    let attributes = attributes(font);
    let families = [Family::Name(&font.family)];
    let query = Query {
        families: &families,
        weight: Weight(attributes.weight),
        stretch: match attributes.stretch {
            "ultra-condensed" => Stretch::UltraCondensed,
            "extra-condensed" => Stretch::ExtraCondensed,
            "semi-condensed" => Stretch::SemiCondensed,
            "condensed" => Stretch::Condensed,
            "semi-expanded" => Stretch::SemiExpanded,
            "expanded" => Stretch::Expanded,
            "extra-expanded" => Stretch::ExtraExpanded,
            "ultra-expanded" => Stretch::UltraExpanded,
            _ => Stretch::Normal,
        },
        style: match attributes.style {
            "italic" => Style::Italic,
            "oblique" => Style::Oblique,
            _ => Style::Normal,
        },
    };
    // Material names cached faces using the font's family and subfamily, not
    // its PostScript name. Preserve that lookup and its sorted-file fallback.
    let faces = database
        .faces()
        .filter_map(|face| {
            database
                .with_face_data(face.id, |data, index| {
                    material_style_name(data, index, &font.family)
                })
                .flatten()
                .map(|name| (name, face.id))
        })
        .collect::<Vec<_>>();
    let requested = if font.variant.is_empty() {
        font.style.clone()
    } else {
        format!("{} {}", font.variant, font.style)
    };
    let id = preferred_face(&requested, &faces)
        .or_else(|| database.query(&query))
        .or_else(|| database.faces().next().map(|face| face.id))
        .with_context(|| {
            format!("font family '{}' has no usable faces", font.family)
        })?;
    let data = database
        .with_face_data(id, |data, _| data.to_vec())
        .context("selected font face has no data")?;
    let mut selected = Database::new();
    selected.load_font_data(data);
    Ok(selected)
}

fn material_style_name(
    data: &[u8], index: u32, family: &str,
) -> Option<String> {
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let name = |id| {
        font.localized_strings(StringId::new(id))
            .english_or_first()
            .map(|value| value.to_string())
    };
    // FreeType (and Pillow) prefers typographic family/subfamily names when
    // both are present, then falls back to the legacy family/style pair.
    let (name, style) =
        name(16).zip(name(17)).or_else(|| name(1).zip(name(2)))?;
    Some(
        format!("{} {style}", name.replace(family, ""))
            .trim()
            .into(),
    )
}

fn preferred_face<T: Copy>(
    requested: &str, faces: &[(String, T)],
) -> Option<T> {
    let mut order = (0..faces.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        format!("{}.ttf", faces[*left].0)
            .cmp(&format!("{}.ttf", faces[*right].0))
    });
    if let Some(index) =
        order.iter().find(|&&index| faces[index].0 == requested)
    {
        return Some(faces[*index].1);
    }
    let mut fallback = *order.first()?;
    for index in order.into_iter().skip(1) {
        if faces[index].0.contains("Regular")
            && faces[index].0.len() < faces[fallback].0.len()
        {
            fallback = index;
        }
    }
    Some(faces[fallback].1)
}

fn validate_font(data: &[u8]) -> Result<()> {
    if data.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || data.starts_with(b"OTTO")
        || data.starts_with(b"ttcf")
        || data.starts_with(b"true")
    {
        Ok(())
    } else {
        bail!("unsupported font data")
    }
}

fn safe_family(family: &str) -> Result<String> {
    let family = family.trim();
    if family.is_empty()
        || family == "."
        || family == ".."
        || family.contains(['/', '\\'])
    {
        bail!("invalid font family '{family}'")
    }
    Ok(family.into())
}

fn read_fonts(directory: &Path) -> Result<Vec<Vec<u8>>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries =
        fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries
        .into_iter()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| matches!(value, "ttf" | "otf"))
        })
        .map(|entry| {
            let path = entry.path();
            let data = fs::read(&path)?;
            validate_font(&data).with_context(|| {
                format!("invalid font in cache at {}", path.display())
            })?;
            Ok(data)
        })
        .collect()
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{attributes, preferred_face, safe_family, validate_font};
    use crate::compat::mkdocs::plugin::social::layout::Font;

    #[test]
    fn validates_font_cache_components() {
        assert_eq!(safe_family("Roboto").unwrap(), "Roboto");
        assert!(safe_family("../font").is_err());
        assert!(safe_family("").is_err());
    }

    #[test]
    fn validates_supported_font_headers() {
        assert!(validate_font(&[0x00, 0x01, 0x00, 0x00]).is_ok());
        assert!(validate_font(b"OTTO").is_ok());
        assert!(validate_font(b"not a font").is_err());
    }

    #[test]
    fn normalizes_font_attributes_without_substring_collisions() {
        let font = Font {
            family: "Roboto".into(),
            variant: "Semi Expanded".into(),
            style: "Extra Light Oblique".into(),
        };
        let attributes = attributes(&font);
        assert_eq!(attributes.weight, 200);
        assert_eq!(attributes.style, "oblique");
        assert_eq!(attributes.stretch, "semi-expanded");
    }

    #[test]
    fn selects_exact_face_or_materials_sorted_fallback() {
        let faces = [
            ("Bold".into(), 0),
            ("Black".into(), 1),
            ("Regular".into(), 2),
            ("Black Italic".into(), 3),
        ];
        assert_eq!(preferred_face("Black Italic", &faces), Some(3));
        assert_eq!(preferred_face("Black Bold", &faces), Some(2));
        assert_eq!(preferred_face("Missing", &faces[..2]), Some(1));
    }
}
