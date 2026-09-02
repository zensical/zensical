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

//! Social card template and image rendering.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use minijinja::{context, AutoEscape, Environment, Value};
use resvg::tiny_skia::{Pixmap, PixmapPaint, Transform};
use resvg::usvg;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::Project;
use crate::structure::dynamic::Dynamic;
use crate::structure::page::Page;

use super::font::{attributes, Fonts};
use super::layout::{Font, Layer, Layout, Typography};
use super::plugin_error;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Rendered social metadata tag.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Tag {
    /// HTML meta property name.
    pub property: String,
    /// Rendered HTML meta content.
    pub content: String,
}

/// Immutable renderer shared by concurrent card jobs.
#[derive(Clone, Debug)]
pub struct Renderer {
    project: std::sync::Arc<Project>,
    theme_dirs: std::sync::Arc<[PathBuf]>,
    fonts: Fonts,
    dependencies: Arc<Mutex<HashMap<PathBuf, [u8; 32]>>>,
}

#[derive(Serialize)]
struct ImageContext<'a> {
    url: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    width: u32,
    height: u32,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Renderer {
    /// Creates one renderer for a plugin instance.
    pub fn new(
        project: std::sync::Arc<Project>, theme_dirs: Vec<PathBuf>,
        cache: PathBuf,
    ) -> Self {
        Self {
            project,
            theme_dirs: theme_dirs.into(),
            fonts: Fonts::new(cache),
            dependencies: Arc::default(),
        }
    }

    /// Resolves all page- and configuration-dependent layer templates.
    pub fn prepare(
        &self, layout: &Layout, page: &Page,
        options: &BTreeMap<String, Dynamic>,
    ) -> Result<Layout> {
        let mut layout = layout.clone();
        let environment = environment();
        let context = template_context(&self.project, page, options, None)?;
        for layer in &mut layout.layers {
            render_layer_templates(layer, &environment, &context)?;
            validate_layer(layer)?;
        }
        Ok(layout)
    }

    /// Hashes physical images and icons referenced by a prepared layout.
    pub fn dependency_revision(
        &self, layout: &Layout, known: &BTreeMap<PathBuf, [u8; 32]>,
    ) -> Result<[u8; 32]> {
        let mut digest = Sha256::new();
        for layer in &layout.layers {
            if !layer.background.image.is_empty() {
                let path = self.background_path(&layer.background.image);
                if !path.is_file() {
                    return Err(plugin_error(anyhow!(
                        "couldn't find image '{}'",
                        path.display()
                    )));
                }
                digest.update(b"background\0");
                digest.update(self.file_revision(&path, known)?);
            }
            if !layer.icon.value.is_empty() {
                let path = self.icon_path(&layer.icon.value)?;
                digest.update(b"icon\0");
                digest.update(self.file_revision(&path, known)?);
            }
        }
        Ok(digest.finalize().into())
    }

    /// Rasterizes one fully prepared card layout.
    pub fn card(
        &self, layout: &Layout, debug: Option<(&str, bool, usize)>,
    ) -> Result<Vec<u8>> {
        let mut card = Pixmap::new(layout.size.width, layout.size.height)
            .context("social card dimensions are too large")?;
        for layer in &layout.layers {
            let image = self.layer(layer)?;
            let (x, y) = offset(layer, layout.size.width, layout.size.height);
            card.draw_pixmap(
                x,
                y,
                image.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        if let Some((color, grid, step)) = debug {
            let svg = debug_svg(layout, color, grid, step)?;
            let fonts = self.fonts.load(&Font {
                family: "Roboto".into(),
                variant: String::new(),
                style: "Regular".into(),
            })?;
            let overlay = render_svg(
                &svg,
                layout.size.width,
                layout.size.height,
                Some(&fonts),
            )?;
            card.draw_pixmap(
                0,
                0,
                overlay.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        card.encode_png().context("failed to encode social card")
    }

    /// Renders the layout-provided Open Graph and Twitter tags.
    pub fn tags(
        &self, layout: &Layout, page: &Page,
        options: &BTreeMap<String, Dynamic>, image_url: &str,
    ) -> Result<Vec<Tag>> {
        let image = ImageContext {
            url: image_url,
            kind: "image/png",
            width: layout.size.width,
            height: layout.size.height,
        };
        let environment = environment();
        let template_data =
            template_context(&self.project, page, options, Some(image))?;
        layout
            .tags
            .iter()
            .filter_map(|(property, source)| {
                let rendered =
                    render_template(&environment, source, &template_data);
                match rendered {
                    Ok(rendered) if rendered.is_empty() => None,
                    Ok(rendered) => Some(Ok(Tag {
                        property: property.clone(),
                        content: rendered,
                    })),
                    Err(error) => Some(Err(error)),
                }
            })
            .collect()
    }

    fn layer(&self, layer: &Layer) -> Result<Pixmap> {
        let fonts = (!layer.typography.content.is_empty())
            .then(|| self.fonts.load(&layer.typography.font))
            .transpose()?;
        let background = if layer.background.image.is_empty() {
            None
        } else {
            Some(self.background(&layer.background.image)?)
        };
        let icon = if layer.icon.value.is_empty() {
            None
        } else {
            Some(self.icon(&layer.icon.value, &layer.icon.color)?)
        };
        let svg = layer_svg(
            layer,
            background.as_deref(),
            icon.as_deref(),
            fonts.as_ref(),
        )?;
        render_svg(&svg, layer.size.width, layer.size.height, fonts.as_ref())
    }

    fn background(&self, value: &str) -> Result<String> {
        let path = self.background_path(value);
        data_url(&path).with_context(|| {
            format!("couldn't find image '{}'", path.display())
        })
    }

    fn background_path(&self, value: &str) -> PathBuf {
        let path = Path::new(value);
        if path.is_absolute() {
            path.to_owned()
        } else {
            self.project.root_dir.join(path)
        }
    }

    fn icon(&self, name: &str, color: &str) -> Result<String> {
        let mut data = self.icon_source(name)?;
        if !color.is_empty() {
            data = colorize_icon(&data, color);
        }
        Ok(format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(data)
        ))
    }

    fn icon_source(&self, name: &str) -> Result<String> {
        let path = self.icon_path(name)?;
        Ok(fs::read_to_string(path)?)
    }

    fn icon_path(&self, name: &str) -> Result<PathBuf> {
        for base in self.theme_dirs.iter() {
            let path = base.join(".icons").join(format!("{name}.svg"));
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => return Ok(path),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(plugin_error(anyhow!("couldn't find icon '{name}'")))
    }

    fn file_revision(
        &self, path: &Path, known: &BTreeMap<PathBuf, [u8; 32]>,
    ) -> Result<[u8; 32]> {
        if let Some(revision) = known.get(path) {
            return Ok(*revision);
        }
        let mut dependencies = self.dependencies.lock().map_err(|_| {
            anyhow!("social dependency cache lock was poisoned")
        })?;
        if let Some(revision) = dependencies.get(path) {
            return Ok(*revision);
        }
        let revision = Sha256::digest(fs::read(path)?).into();
        dependencies.insert(path.to_owned(), revision);
        Ok(revision)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn template_context(
    project: &Project, page: &Page, options: &BTreeMap<String, Dynamic>,
    image: Option<ImageContext<'_>>,
) -> Result<Value> {
    let is_homepage = page.source().parent().is_none()
        && matches!(page.source().file_name(), "index.md" | "README.md");
    let mut page_value = serde_json::to_value(page)?;
    let page_map = page_value
        .as_object_mut()
        .context("serialized page must be a mapping")?;
    page_map.insert("is_homepage".into(), serde_json::Value::Bool(is_homepage));
    Ok(context! {
        config => project,
        page => page_value,
        layout => options,
        image => image,
    })
}

fn environment() -> Environment<'static> {
    let mut environment = Environment::new();
    environment.set_auto_escape_callback(|_| AutoEscape::None);
    environment.set_unknown_method_callback(
        minijinja_contrib::pycompat::unknown_method_callback,
    );
    environment.add_filter("x", |value: Value| {
        if value.is_undefined() || value.is_none() || !value.is_true() {
            Value::from("")
        } else {
            value
        }
    });
    environment
}

fn render_template(
    environment: &Environment<'_>, source: &str, context: &Value,
) -> Result<String> {
    let source = html_escape::decode_html_entities(source);
    environment
        .render_str(&source, context)
        .map(|value| value.trim().to_owned())
        .context("failed to render social layout expression")
}

fn render_layer_templates(
    layer: &mut Layer, environment: &Environment<'_>, context: &Value,
) -> Result<()> {
    layer.origin = render_template(environment, &layer.origin, context)?;
    layer.background.color =
        render_template(environment, &layer.background.color, context)?;
    layer.background.image =
        render_template(environment, &layer.background.image, context)?;
    layer.icon.value =
        render_template(environment, &layer.icon.value, context)?;
    layer.icon.color =
        render_template(environment, &layer.icon.color, context)?;
    layer.typography.content =
        render_template(environment, &layer.typography.content, context)?;
    layer.typography.align =
        render_template(environment, &layer.typography.align, context)?;
    layer.typography.overflow =
        render_template(environment, &layer.typography.overflow, context)?;
    layer.typography.color =
        render_template(environment, &layer.typography.color, context)?;
    layer.typography.font.family =
        render_template(environment, &layer.typography.font.family, context)?;
    layer.typography.font.variant =
        render_template(environment, &layer.typography.font.variant, context)?;
    layer.typography.font.style =
        render_template(environment, &layer.typography.font.style, context)?;
    Ok(())
}

fn validate_layer(layer: &Layer) -> Result<()> {
    if !layer.background.color.is_empty() {
        parse_color(&layer.background.color, "background color")?;
    }
    if !layer.icon.color.is_empty() {
        parse_color(&layer.icon.color, "icon color")?;
    }
    if !layer.typography.content.is_empty() {
        parse_color(&layer.typography.color, "typography color")?;
    }
    Ok(())
}

fn parse_color(value: &str, name: &str) -> Result<svgtypes::Color> {
    value
        .parse()
        .with_context(|| format!("invalid social card {name}: {value}"))
}

fn layer_svg(
    layer: &Layer, background: Option<&str>, icon: Option<&str>,
    fonts: Option<&Arc<usvg::fontdb::Database>>,
) -> Result<String> {
    let mut body = String::new();
    if let Some(background) = background {
        write!(
            body,
            "<image width=\"{}\" height=\"{}\" href=\"{}\" preserveAspectRatio=\"xMidYMid slice\"/>",
            layer.size.width,
            layer.size.height,
            xml_attribute(background),
        )
        .expect("writing to a string cannot fail");
    }
    if !layer.background.color.is_empty()
        && layer.background.color != "transparent"
    {
        write!(
            body,
            "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>",
            xml_attribute(&layer.background.color),
        )
        .expect("writing to a string cannot fail");
    }
    if let Some(icon) = icon {
        write!(
            body,
            "<image width=\"{}\" height=\"{}\" href=\"{}\" preserveAspectRatio=\"xMidYMid meet\"/>",
            layer.size.width,
            layer.size.height,
            xml_attribute(icon),
        )
        .expect("writing to a string cannot fail");
    }
    if !layer.typography.content.is_empty() {
        body.push_str(&typography_svg(&layer.typography, layer, fonts)?);
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">{body}</svg>",
        layer.size.width, layer.size.height, layer.size.width, layer.size.height,
    ))
}

fn typography_svg(
    typography: &Typography, layer: &Layer,
    fonts: Option<&Arc<usvg::fontdb::Database>>,
) -> Result<String> {
    let mut allowed = typography.line.amount;
    let mut size =
        font_size(layer.size.height, allowed, typography.line.height);
    let mut lines = wrap(
        &typography.content,
        f64::from(layer.size.width),
        size,
        &typography.font,
        fonts,
    )?;
    if typography.overflow == "shrink" {
        while lines.len() > allowed {
            allowed += 1;
            size =
                font_size(layer.size.height, allowed, typography.line.height);
            lines = wrap(
                &typography.content,
                f64::from(layer.size.width),
                size,
                &typography.font,
                fonts,
            )?;
        }
        balance_two_lines(&mut lines);
    } else if lines.len() > allowed {
        lines.truncate(allowed);
        let last = lines.last_mut().expect("at least one line");
        let appended = format!("{last} ...");
        if measure(&appended, size, &typography.font, fonts)?
            <= f64::from(layer.size.width)
        {
            *last = appended;
        } else {
            while let Some((head, _)) = last.rsplit_once(' ') {
                *last = head.into();
                if measure(
                    &format!("{last} ..."),
                    size,
                    &typography.font,
                    fonts,
                )? <= f64::from(layer.size.width)
                {
                    break;
                }
            }
            *last = if last.is_empty() {
                "...".into()
            } else {
                format!("{last} ...")
            };
        }
    } else {
        balance_two_lines(&mut lines);
    }

    let line_height = size * typography.line.height;
    let additional = u32::try_from(lines.len().saturating_sub(1))
        .map_or(f64::from(u32::MAX), f64::from);
    let height = size + line_height * additional;
    let (anchor, x) =
        horizontal(&typography.align, f64::from(layer.size.width));
    let y = vertical(&typography.align, f64::from(layer.size.height), height);
    let attributes = font_attributes(&typography.font);
    let mut spans = String::new();
    for (index, line) in lines.iter().enumerate() {
        let dy = if index == 0 { 0.0 } else { line_height };
        write!(
            spans,
            "<tspan x=\"{x}\" dy=\"{dy}\">{}</tspan>",
            xml_text(line),
        )
        .expect("writing to a string cannot fail");
    }
    Ok(format!(
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"{anchor}\" dominant-baseline=\"hanging\" fill=\"{}\" font-family=\"{}\" font-size=\"{size}\" {attributes}>{spans}</text>",
        xml_attribute(&typography.color),
        xml_attribute(&typography.font.family),
    ))
}

fn wrap(
    content: &str, width: f64, size: f64, font: &Font,
    fonts: Option<&Arc<usvg::fontdb::Database>>,
) -> Result<Vec<String>> {
    let content = html_escape::decode_html_entities(content);
    let words = content.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return Ok(vec![String::new()]);
    }
    let mut lines = vec![String::new()];
    for word in words {
        let line = lines.last_mut().expect("one line exists");
        let candidate = if line.is_empty() {
            word.into()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && measure(&candidate, size, font, fonts)? > width {
            lines.push(word.into());
        } else {
            *line = candidate;
        }
    }
    Ok(lines)
}

fn measure(
    text: &str, size: f64, font: &Font,
    fonts: Option<&Arc<usvg::fontdb::Database>>,
) -> Result<f64> {
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100000\" height=\"4096\"><text x=\"0\" y=\"2048\" font-family=\"{}\" font-size=\"{size}\" {}>{}</text></svg>",
        xml_attribute(&font.family),
        font_attributes(font),
        xml_text(text),
    );
    let tree = parse_svg(&svg, fonts)?;
    Ok(f64::from(tree.root().abs_layer_bounding_box().width()))
}

fn parse_svg(
    source: &str, fonts: Option<&Arc<usvg::fontdb::Database>>,
) -> Result<usvg::Tree> {
    let mut options = usvg::Options::default();
    if let Some(fonts) = fonts {
        options.fontdb = Arc::clone(fonts);
    }
    usvg::Tree::from_data(source.as_bytes(), &options)
        .context("failed to parse generated social card SVG")
}

fn render_svg(
    source: &str, width: u32, height: u32,
    fonts: Option<&Arc<usvg::fontdb::Database>>,
) -> Result<Pixmap> {
    let tree = parse_svg(source, fonts)?;
    let mut pixmap =
        Pixmap::new(width, height).context("social layer is too large")?;
    resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());
    Ok(pixmap)
}

fn font_size(height: u32, lines: usize, line_height: f64) -> f64 {
    let lines = u32::try_from(lines).map_or(f64::from(u32::MAX), f64::from);
    let extent = lines + 0.25 + (lines - 1.0) * (line_height - 1.0);
    f64::from(height) / extent
}

fn font_attributes(font: &Font) -> String {
    let attributes = attributes(font);
    format!(
        "font-weight=\"{}\" font-style=\"{}\" font-stretch=\"{}\"",
        attributes.weight, attributes.style, attributes.stretch,
    )
}

fn balance_two_lines(lines: &mut [String]) {
    if lines.len() != 2 {
        return;
    }
    let Some((head, word)) = lines[0].rsplit_once(' ') else {
        return;
    };
    let before = lines[0].len().abs_diff(lines[1].len());
    let second = format!("{word} {}", lines[1]);
    let after = head.len().abs_diff(second.len());
    if after < before {
        lines[0] = head.into();
        lines[1] = second;
    }
}

fn horizontal(align: &str, width: f64) -> (&'static str, f64) {
    let words = align.split_whitespace().collect::<Vec<_>>();
    if words.contains(&"start") {
        ("start", 0.0)
    } else if words.contains(&"end") {
        ("end", width)
    } else if words.contains(&"center") {
        ("middle", width / 2.0)
    } else {
        ("start", 0.0)
    }
}

fn vertical(align: &str, height: f64, text_height: f64) -> f64 {
    let words = align.split_whitespace().collect::<Vec<_>>();
    if words.contains(&"top") {
        0.0
    } else if words.contains(&"bottom") {
        height - text_height
    } else if words.contains(&"center") {
        (height - text_height) / 2.0
    } else {
        0.0
    }
}

fn offset(layer: &Layer, width: u32, height: u32) -> (i32, i32) {
    let words = layer.origin.split_whitespace().collect::<Vec<_>>();
    let mut x = i64::from(layer.offset.x);
    let mut y = i64::from(layer.offset.y);
    if !words.contains(&"start") {
        if words.contains(&"end") {
            x = i64::from(width) - i64::from(layer.size.width) - x;
        } else if words.contains(&"center") {
            x += (i64::from(width) - i64::from(layer.size.width)) / 2;
        }
    }
    if !words.contains(&"top") {
        if words.contains(&"bottom") {
            y = i64::from(height) - i64::from(layer.size.height) - y;
        } else if words.contains(&"center") {
            y += (i64::from(height) - i64::from(layer.size.height)) / 2;
        }
    }
    (clamp_coordinate(x), clamp_coordinate(y))
}

fn clamp_coordinate(value: i64) -> i32 {
    i32::try_from(value).unwrap_or_else(|_| {
        if value.is_negative() {
            i32::MIN
        } else {
            i32::MAX
        }
    })
}

fn data_url(path: &Path) -> Result<String> {
    let data = fs::read(path)?;
    let kind = match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => bail!("unsupported social background format: {}", path.display()),
    };
    Ok(format!(
        "data:{kind};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(data)
    ))
}

fn colorize_icon(source: &str, color: &str) -> String {
    let color = xml_attribute(color);
    if source.contains("currentColor") {
        return source.replace("currentColor", &color);
    }
    let Some(end) = source.find('>') else {
        return source.into();
    };
    let (root, body) = source.split_at(end);
    let root = if root.contains("fill=\"none\"") {
        root.replacen("fill=\"none\"", &format!("fill=\"{color}\""), 1)
    } else if root.contains("fill='none'") {
        root.replacen("fill='none'", &format!("fill='{color}'"), 1)
    } else if root.contains(" fill=") {
        root.into()
    } else {
        format!("{root} fill=\"{color}\"")
    };
    format!("{root}{body}")
}

fn debug_svg(
    layout: &Layout, color: &str, grid: bool, step: usize,
) -> Result<String> {
    let parsed = parse_color(color, "debug color")?;
    let label_color = if f64::from(parsed.red) * 0.299
        + f64::from(parsed.green) * 0.587
        + f64::from(parsed.blue) * 0.114
        > 150.0
    {
        "black"
    } else {
        "white"
    };
    let mut body = String::new();
    if grid {
        for x in (0..layout.size.width).step_by(step) {
            for y in (0..layout.size.height).step_by(step) {
                write!(
                    body,
                    "<circle cx=\"{x}\" cy=\"{y}\" r=\"1\" fill=\"{}\"/>",
                    xml_attribute(color),
                )
                .expect("writing to a string cannot fail");
            }
        }
    }
    for (index, layer) in layout.layers.iter().enumerate() {
        let (x, y) = offset(layer, layout.size.width, layout.size.height);
        write!(
            body,
            "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{}\"/>",
            layer.size.width,
            layer.size.height,
            xml_attribute(color),
        )
        .expect("writing to a string cannot fail");
        let label = format!("{index} – {x}, {y}");
        let width = 12_u32.saturating_mul(
            u32::try_from(label.chars().count()).unwrap_or(u32::MAX),
        );
        write!(
            body,
            "<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"20\" fill=\"{}\"/><text x=\"{}\" y=\"{}\" fill=\"{label_color}\" font-family=\"Roboto\" font-size=\"12\" dominant-baseline=\"hanging\">{}</text>",
            xml_attribute(color),
            x.saturating_add(4),
            y.saturating_add(3),
            xml_text(&label),
        )
        .expect("writing to a string cannot fail");
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">{body}</svg>",
        layout.size.width, layout.size.height,
    ))
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml_attribute(value: &str) -> String {
    xml_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::{
        balance_two_lines, colorize_icon, debug_svg, environment,
        font_attributes, layer_svg, offset, render_svg, render_template, Font,
        Layer, Layout,
    };
    use minijinja::Value;

    #[test]
    fn balances_two_lines_by_moving_one_word() {
        let mut lines = vec!["one two three".into(), "four".into()];
        balance_two_lines(&mut lines);
        assert_eq!(lines, ["one two", "three four"]);
    }

    #[test]
    fn maps_font_style_to_svg_attributes() {
        let font = Font {
            family: "Roboto".into(),
            variant: "Condensed".into(),
            style: "Bold Italic".into(),
        };
        let attributes = font_attributes(&font);
        assert!(attributes.contains("font-weight=\"700\""));
        assert!(attributes.contains("font-style=\"italic\""));
        assert!(attributes.contains("font-stretch=\"condensed\""));
    }

    #[test]
    fn renders_embedded_svg_icons() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 10 10"><path d="M1 1L9 9"/></svg>"#;
        let source = colorize_icon(source, "white");
        let icon = format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(source)
        );
        let mut layer = Layer::default();
        layer.size.width = 10;
        layer.size.height = 10;
        let svg = layer_svg(&layer, None, Some(&icon), None).unwrap();
        let pixmap = render_svg(&svg, 10, 10, None).unwrap();

        assert!(pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn decodes_html_entities_before_rendering_templates() {
        let rendered =
            render_template(&environment(), "A &amp; B", &Value::UNDEFINED)
                .unwrap();
        assert_eq!(rendered, "A & B");
    }

    #[test]
    fn computes_signed_offsets_for_oversized_layers() {
        let mut layer = Layer::default();
        layer.size.width = 20;
        layer.size.height = 20;
        layer.origin = "end bottom".into();
        assert_eq!(offset(&layer, 10, 10), (-10, -10));
    }

    #[test]
    fn debug_labels_contrast_with_the_overlay() {
        let layout = Layout {
            tags: Vec::new(),
            size: super::super::layout::Size { width: 10, height: 10 },
            layers: vec![Layer::default()],
        };
        assert!(debug_svg(&layout, "white", false, 1)
            .unwrap()
            .contains("fill=\"black\""));
        assert!(debug_svg(&layout, "not-a-color", false, 1).is_err());
    }
}
