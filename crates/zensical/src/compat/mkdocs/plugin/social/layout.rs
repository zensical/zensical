// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Social card layout model and YAML parser.

use anyhow::{bail, Context, Result};
use saphyr::{LoadableYamlNode, YamlOwned};
use serde::Serialize;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

const ORIGINS: &[&str] = &[
    "start top",
    "center top",
    "end top",
    "start center",
    "center",
    "end center",
    "start bottom",
    "center bottom",
    "end bottom",
    "start",
    "end",
];

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Social card layout.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct Layout {
    pub tags: Vec<(String, String)>,
    pub size: Size,
    pub layers: Vec<Layer>,
}

/// Pixel dimensions.
#[derive(Clone, Copy, Debug, Default, Hash, Serialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// Signed layer offset.
#[derive(Clone, Copy, Debug, Default, Hash, Serialize)]
pub struct Offset {
    pub x: i32,
    pub y: i32,
}

/// One composited card layer.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct Layer {
    pub size: Size,
    pub offset: Offset,
    pub origin: String,
    pub background: Background,
    pub icon: Icon,
    pub typography: Typography,
}

/// Layer background.
#[derive(Clone, Debug, Default, Hash, Serialize)]
pub struct Background {
    pub color: String,
    pub image: String,
}

/// Layer icon.
#[derive(Clone, Debug, Default, Hash, Serialize)]
pub struct Icon {
    pub value: String,
    pub color: String,
}

/// Layer typography.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct Typography {
    pub content: String,
    pub align: String,
    pub overflow: String,
    pub color: String,
    pub line: Line,
    pub font: Font,
}

/// Typography line settings.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Line {
    pub amount: usize,
    pub height: f64,
}

/// Typography font settings.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize)]
pub struct Font {
    pub family: String,
    pub variant: String,
    pub style: String,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl std::hash::Hash for Line {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.amount.hash(state);
        self.height.to_bits().hash(state);
    }
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            size: Size::default(),
            offset: Offset::default(),
            origin: "start top".into(),
            background: Background::default(),
            icon: Icon::default(),
            typography: Typography::default(),
        }
    }
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            content: String::new(),
            align: "start top".into(),
            overflow: "truncate".into(),
            color: String::new(),
            line: Line { amount: 1, height: 1.0 },
            font: Font {
                family: "Roboto".into(),
                variant: String::new(),
                style: "Regular".into(),
            },
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Parses and validates one complete layout document.
pub fn parse(path: &str, source: &str) -> Result<Layout> {
    let documents =
        YamlOwned::load_from_str(source.trim_start_matches('\u{feff}'))
            .with_context(|| format!("error reading social layout '{path}'"))?;
    if documents.len() != 1 {
        bail!("social layout must contain exactly one YAML document [{path}]")
    }
    parse_layout(&documents[0])
        .with_context(|| format!("error reading social layout '{path}'"))
}

fn parse_layout(node: &YamlOwned) -> Result<Layout> {
    let mut size = Size::default();
    let mut tags = Vec::new();
    let mut layers = Vec::new();
    for (key, value) in mapping(node, "layout")? {
        match string(key, "layout option")? {
            "definitions" => {
                for definition in sequence(value, "definitions")? {
                    string(definition, "definition")?;
                }
            }
            "tags" => tags = parse_tags(value)?,
            "size" => size = parse_size(value, "size")?,
            "layers" => {
                layers = sequence(value, "layers")?
                    .iter()
                    .map(parse_layer)
                    .collect::<Result<_>>()?;
            }
            key => bail!("unknown layout option: {key}"),
        }
    }
    if size.width == 0 || size.height == 0 {
        bail!("layout width and height must be greater than zero")
    }
    for layer in &mut layers {
        if layer.size.width == 0 {
            layer.size.width = size.width;
        }
        if layer.size.height == 0 {
            layer.size.height = size.height;
        }
    }
    Ok(Layout { tags, size, layers })
}

fn parse_tags(node: &YamlOwned) -> Result<Vec<(String, String)>> {
    mapping(node, "tags")?
        .into_iter()
        .map(|(key, value)| {
            Ok((
                string(key, "tag name")?.into(),
                owned_string(value, "tag value")?,
            ))
        })
        .collect()
}

fn parse_layer(node: &YamlOwned) -> Result<Layer> {
    let mut layer = Layer::default();
    for (key, value) in mapping(node, "layer")? {
        match string(key, "layer option")? {
            "size" => layer.size = parse_size(value, "layer.size")?,
            "offset" => layer.offset = parse_offset(value)?,
            "origin" => {
                layer.origin = choice(value, "origin", ORIGINS)?;
            }
            "background" => layer.background = parse_background(value)?,
            "icon" => layer.icon = parse_icon(value)?,
            "typography" => layer.typography = parse_typography(value)?,
            key => bail!("unknown layer option: {key}"),
        }
    }
    Ok(layer)
}

fn parse_size(node: &YamlOwned, name: &str) -> Result<Size> {
    let mut size = Size::default();
    for (key, value) in mapping(node, name)? {
        match string(key, "size option")? {
            "width" => size.width = unsigned(value, "width")?,
            "height" => size.height = unsigned(value, "height")?,
            key => bail!("unknown size option: {key}"),
        }
    }
    Ok(size)
}

fn parse_offset(node: &YamlOwned) -> Result<Offset> {
    let mut offset = Offset::default();
    for (key, value) in mapping(node, "offset")? {
        match string(key, "offset option")? {
            "x" => offset.x = integer(value, "offset.x")?,
            "y" => offset.y = integer(value, "offset.y")?,
            key => bail!("unknown offset option: {key}"),
        }
    }
    Ok(offset)
}

fn parse_background(node: &YamlOwned) -> Result<Background> {
    let mut background = Background::default();
    for (key, value) in mapping(node, "background")? {
        match string(key, "background option")? {
            "color" => background.color = owned_string(value, "color")?,
            "image" => background.image = owned_string(value, "image")?,
            key => bail!("unknown background option: {key}"),
        }
    }
    Ok(background)
}

fn parse_icon(node: &YamlOwned) -> Result<Icon> {
    let mut icon = Icon::default();
    for (key, value) in mapping(node, "icon")? {
        match string(key, "icon option")? {
            "value" => icon.value = owned_string(value, "value")?,
            "color" => icon.color = owned_string(value, "color")?,
            key => bail!("unknown icon option: {key}"),
        }
    }
    Ok(icon)
}

fn parse_typography(node: &YamlOwned) -> Result<Typography> {
    let mut typography = Typography::default();
    for (key, value) in mapping(node, "typography")? {
        match string(key, "typography option")? {
            "content" => {
                typography.content = owned_string(value, "content")?;
            }
            "align" => {
                typography.align = choice(value, "align", ORIGINS)?;
            }
            "overflow" => {
                typography.overflow =
                    choice(value, "overflow", &["truncate", "shrink"])?;
            }
            "color" => {
                typography.color = owned_string(value, "color")?;
            }
            "line" => typography.line = parse_line(value)?,
            "font" => typography.font = parse_font(value)?,
            key => bail!("unknown typography option: {key}"),
        }
    }
    if typography.line.amount == 0
        || !typography.line.height.is_finite()
        || typography.line.height <= 0.0
    {
        bail!("typography line amount and height must be greater than zero")
    }
    Ok(typography)
}

fn parse_line(node: &YamlOwned) -> Result<Line> {
    let mut line = Typography::default().line;
    for (key, value) in mapping(node, "line")? {
        match string(key, "line option")? {
            "amount" => line.amount = positive_usize(value, "line.amount")?,
            "height" => line.height = number(value, "line.height")?,
            key => bail!("unknown line option: {key}"),
        }
    }
    Ok(line)
}

fn parse_font(node: &YamlOwned) -> Result<Font> {
    let mut font = Typography::default().font;
    for (key, value) in mapping(node, "font")? {
        match string(key, "font option")? {
            "family" => font.family = owned_string(value, "family")?,
            "variant" => font.variant = owned_string(value, "variant")?,
            "style" => font.style = owned_string(value, "style")?,
            key => bail!("unknown font option: {key}"),
        }
    }
    Ok(font)
}

fn mapping<'a>(
    node: &'a YamlOwned, name: &str,
) -> Result<Vec<(&'a YamlOwned, &'a YamlOwned)>> {
    let values = node
        .as_mapping()
        .with_context(|| format!("{name} must be a mapping"))?;
    let mut inherited = Vec::new();
    let mut explicit = Vec::new();
    for (key, value) in values {
        if key.as_str() == Some("<<") {
            merge(&mut inherited, value, name)?;
        } else {
            explicit.push((key, value));
        }
    }
    for (key, value) in explicit {
        if let Some(entry) = inherited
            .iter_mut()
            .find(|(inherited, _)| inherited == &key)
        {
            *entry = (key, value);
        } else {
            inherited.push((key, value));
        }
    }
    Ok(inherited)
}

fn merge<'a>(
    target: &mut Vec<(&'a YamlOwned, &'a YamlOwned)>, node: &'a YamlOwned,
    name: &str,
) -> Result<()> {
    if let Some(values) = node.as_mapping() {
        for entry in values {
            if !target.iter().any(|(key, _)| key == &entry.0) {
                target.push(entry);
            }
        }
        return Ok(());
    }
    if let Some(values) = node.as_sequence() {
        for value in values {
            merge(target, value, name)?;
        }
        return Ok(());
    }
    bail!("{name} merge value must be a mapping or list of mappings")
}

fn sequence<'a>(node: &'a YamlOwned, name: &str) -> Result<&'a [YamlOwned]> {
    node.as_sequence()
        .map(Vec::as_slice)
        .with_context(|| format!("{name} must be a list"))
}

fn string<'a>(node: &'a YamlOwned, name: &str) -> Result<&'a str> {
    node.as_str()
        .with_context(|| format!("{name} must be a string"))
}

fn owned_string(node: &YamlOwned, name: &str) -> Result<String> {
    string(node, name).map(Into::into)
}

fn integer(node: &YamlOwned, name: &str) -> Result<i32> {
    node.as_integer()
        .and_then(|value| i32::try_from(value).ok())
        .with_context(|| format!("{name} must be an integer"))
}

fn unsigned(node: &YamlOwned, name: &str) -> Result<u32> {
    node.as_integer()
        .and_then(|value| u32::try_from(value).ok())
        .with_context(|| format!("{name} must be a non-negative integer"))
}

fn number(node: &YamlOwned, name: &str) -> Result<f64> {
    node.as_floating_point()
        .or_else(|| {
            node.as_integer()
                .and_then(|value| value.to_string().parse().ok())
        })
        .with_context(|| format!("{name} must be a number"))
}

fn positive_usize(node: &YamlOwned, name: &str) -> Result<usize> {
    if let Some(value) = node.as_integer() {
        return usize::try_from(value)
            .ok()
            .filter(|value| *value > 0)
            .with_context(|| format!("{name} must be a positive integer"));
    }
    let value = node
        .as_floating_point()
        .with_context(|| format!("{name} must be a positive integer"))?;
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 {
        bail!("{name} must be a positive integer")
    }
    value
        .to_string()
        .parse()
        .with_context(|| format!("{name} must be a positive integer"))
}

fn choice(node: &YamlOwned, name: &str, values: &[&str]) -> Result<String> {
    let value = string(node, name)?;
    if values.contains(&value) {
        Ok(value.into())
    } else {
        bail!("invalid {name}: {value}")
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_anchors_and_defaults_layer_size() {
        let layout = parse(
            "custom.yml",
            "definitions: [&value '#fff']\ntags: { og:title: title }\nsize: { width: 1200, height: 630 }\nlayers:\n  - background: { color: *value }\n",
        )
        .unwrap();
        assert_eq!(layout.layers[0].size.width, 1200);
        assert_eq!(layout.layers[0].background.color, "#fff");
    }

    #[test]
    fn expands_yaml_merge_keys() {
        let layout = parse(
            "custom.yml",
            "size: { width: 1200, height: 630 }\nlayers:\n  - &layer\n    background: { color: '#fff' }\n  - <<: *layer\n    origin: center\n",
        )
        .unwrap();
        assert_eq!(layout.layers[1].background.color, "#fff");
        assert_eq!(layout.layers[1].origin, "center");
    }

    #[test]
    fn rejects_invalid_layouts() {
        assert!(parse("bad.yml", "size: { width: 0, height: 2 }\n").is_err());
        assert!(parse("bad.yml", "size: { width: 2, height: 2 }\nwat: 1\n")
            .is_err());
        assert!(parse(
            "bad.yml",
            "definitions: [{ bad: value }]\nsize: { width: 2, height: 2 }\n"
        )
        .is_err());
        assert!(parse(
            "bad.yml",
            "size: { width: 2, height: 2 }\nlayers: [{ background: { color: null } }]\n"
        )
        .is_err());
    }
}
