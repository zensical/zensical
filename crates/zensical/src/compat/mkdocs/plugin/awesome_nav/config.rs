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

//! Strict `.nav.yml` parsing and configuration models.

use anyhow::{bail, Context, Result};
use saphyr::{LoadableYamlNode, YamlOwned};

/// One directory configuration before inheritance is applied.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub title: Option<String>,
    pub hide: bool,
    pub flatten_single_child_sections: Option<bool>,
    pub preserve_directory_names: Option<bool>,
    pub use_index_title: Option<bool>,
    pub sort: Sort,
    pub ignore: Option<Vec<String>>,
    pub nav: Option<Vec<Item>>,
    pub append_unmatched: Option<bool>,
}

/// One configured navigation entry.
#[derive(Clone, Debug)]
pub enum Item {
    Target(String),
    Named { title: String, value: Named },
    Pattern(PatternOptions),
}

/// Value of a named navigation entry.
#[derive(Clone, Debug)]
pub enum Named {
    Target(String),
    Children(Vec<Item>),
}

/// Options local to one pattern.
#[derive(Clone, Debug)]
pub struct PatternOptions {
    pub glob: String,
    pub flatten_single_child_sections: Option<bool>,
    pub preserve_directory_names: Option<bool>,
    pub sort: Sort,
    pub ignore: Option<Vec<String>>,
    pub append_unmatched: Option<bool>,
    pub ignore_no_matches: bool,
}

/// Inheritable sorting options.
#[derive(Clone, Debug, Default)]
pub struct Sort {
    pub by: Option<SortBy>,
    pub direction: Option<Direction>,
    pub kind: Option<SortKind>,
    pub sections: Option<Sections>,
    pub ignore_case: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Path,
    Filename,
    Title,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKind {
    Natural,
    Alphabetical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sections {
    First,
    Last,
    Mixed,
}

/// Parses one complete awesome-nav configuration document.
pub fn parse(path: &str, source: &str) -> Result<Config> {
    let documents =
        YamlOwned::load_from_str(source.trim_start_matches('\u{feff}'))
            .with_context(|| format!("Parsing error [{path}]"))?;
    if documents.len() != 1 {
        bail!(
            "awesome-nav configuration must contain one YAML document [{path}]"
        )
    }
    let root = &documents[0];
    if root.is_null() {
        return Ok(Config::default());
    }
    parse_config(root).with_context(|| format!("Validation error [{path}]"))
}

fn parse_config(node: &YamlOwned) -> Result<Config> {
    let mapping = mapping(node, "configuration root")?;
    let mut config = Config::default();
    for (key, value) in mapping {
        match string(key, "configuration key")? {
            "title" => config.title = Some(non_empty(value, "title")?),
            "hide" => config.hide = boolean(value, "hide")?,
            "flatten_single_child_sections" => {
                config.flatten_single_child_sections =
                    Some(boolean(value, "flatten_single_child_sections")?);
            }
            "preserve_directory_names" => {
                config.preserve_directory_names =
                    Some(boolean(value, "preserve_directory_names")?);
            }
            "use_index_title" => {
                config.use_index_title =
                    Some(boolean(value, "use_index_title")?);
            }
            "sort" => config.sort = parse_sort(value)?,
            "ignore" => config.ignore = Some(parse_ignore(value)?),
            "nav" => config.nav = Some(parse_items(value)?),
            "append_unmatched" => {
                config.append_unmatched =
                    Some(boolean(value, "append_unmatched")?);
            }
            key => bail!("unknown awesome-nav option: {key}"),
        }
    }
    Ok(config)
}

fn parse_items(node: &YamlOwned) -> Result<Vec<Item>> {
    sequence(node, "nav")?.iter().map(parse_item).collect()
}

fn parse_item(node: &YamlOwned) -> Result<Item> {
    if let Some(value) = node.as_str() {
        if value.is_empty() {
            bail!("nav entries must not be empty")
        }
        return Ok(Item::Target(value.into()));
    }
    let mapping = mapping(node, "nav entry")?;
    if mapping.keys().any(|key| key.as_str() == Some("glob")) {
        return parse_pattern(mapping).map(Item::Pattern);
    }
    if mapping.len() != 1 {
        bail!("named nav entries must contain exactly one item")
    }
    let (key, value) = mapping.iter().next().expect("length checked");
    let title = non_empty(key, "nav title")?;
    let value = if let Some(target) = value.as_str() {
        if target.is_empty() {
            bail!("nav targets must not be empty")
        }
        Named::Target(target.into())
    } else {
        Named::Children(parse_items(value)?)
    };
    Ok(Item::Named { title, value })
}

fn parse_pattern(mapping: &saphyr::MappingOwned) -> Result<PatternOptions> {
    let mut glob = None;
    let mut options = PatternOptions {
        glob: String::new(),
        flatten_single_child_sections: None,
        preserve_directory_names: None,
        sort: Sort::default(),
        ignore: None,
        append_unmatched: None,
        ignore_no_matches: false,
    };
    for (key, value) in mapping {
        match string(key, "pattern option")? {
            "glob" => glob = Some(non_empty(value, "glob")?),
            "flatten_single_child_sections" => {
                options.flatten_single_child_sections =
                    Some(boolean(value, "flatten_single_child_sections")?);
            }
            "preserve_directory_names" => {
                options.preserve_directory_names =
                    Some(boolean(value, "preserve_directory_names")?);
            }
            "sort" => options.sort = parse_sort(value)?,
            "ignore" => options.ignore = Some(parse_ignore(value)?),
            "append_unmatched" => {
                options.append_unmatched =
                    Some(boolean(value, "append_unmatched")?);
            }
            "ignore_no_matches" => {
                options.ignore_no_matches =
                    boolean(value, "ignore_no_matches")?;
            }
            key => bail!("unknown pattern option: {key}"),
        }
    }
    options.glob = glob.context("pattern options require 'glob'")?;
    Ok(options)
}

fn parse_sort(node: &YamlOwned) -> Result<Sort> {
    let mut sort = Sort::default();
    for (key, value) in mapping(node, "sort")? {
        match string(key, "sort option")? {
            "by" => {
                sort.by = Some(match string(value, "sort.by")? {
                    "path" => SortBy::Path,
                    "filename" => SortBy::Filename,
                    "title" => SortBy::Title,
                    value => bail!("invalid sort.by value: {value}"),
                });
            }
            "direction" => {
                sort.direction = Some(match string(value, "sort.direction")? {
                    "asc" => Direction::Ascending,
                    "desc" => Direction::Descending,
                    value => bail!("invalid sort.direction value: {value}"),
                });
            }
            "type" => {
                sort.kind = Some(match string(value, "sort.type")? {
                    "natural" => SortKind::Natural,
                    "alphabetical" => SortKind::Alphabetical,
                    value => bail!("invalid sort.type value: {value}"),
                });
            }
            "sections" => {
                sort.sections = Some(match string(value, "sort.sections")? {
                    "first" => Sections::First,
                    "last" => Sections::Last,
                    "mixed" => Sections::Mixed,
                    value => bail!("invalid sort.sections value: {value}"),
                });
            }
            "ignore_case" => {
                sort.ignore_case = Some(boolean(value, "sort.ignore_case")?);
            }
            key => bail!("unknown sort option: {key}"),
        }
    }
    Ok(sort)
}

fn parse_ignore(node: &YamlOwned) -> Result<Vec<String>> {
    if let Some(value) = node.as_str() {
        if value.is_empty() {
            bail!("ignore patterns must not be empty")
        }
        return Ok(vec![value.into()]);
    }
    sequence(node, "ignore")?
        .iter()
        .map(|value| non_empty(value, "ignore pattern"))
        .collect()
}

fn mapping<'a>(
    node: &'a YamlOwned, name: &str,
) -> Result<&'a saphyr::MappingOwned> {
    node.as_mapping()
        .with_context(|| format!("{name} must be a mapping"))
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

fn non_empty(node: &YamlOwned, name: &str) -> Result<String> {
    let value = string(node, name)?;
    if value.is_empty() {
        bail!("{name} must not be empty")
    }
    Ok(value.into())
}

fn boolean(node: &YamlOwned, name: &str) -> Result<bool> {
    node.as_bool()
        .with_context(|| format!("{name} must be a boolean"))
}

#[cfg(test)]
mod tests {
    use super::{parse, Item, Named, Sections, SortBy};

    #[test]
    fn parses_complete_configuration() {
        let config = parse(
            ".nav.yml",
            "title: Guide\nhide: true\nsort:\n  by: title\n  sections: first\nignore: [$inherit, '*.hidden.md']\nnav:\n  - Home: index.md\n  - API:\n      - api.md\n  - glob: '*.md'\n    ignore_no_matches: true\n",
        )
        .unwrap();
        assert_eq!(config.title.as_deref(), Some("Guide"));
        assert!(config.hide);
        assert_eq!(config.sort.by, Some(SortBy::Title));
        assert_eq!(config.sort.sections, Some(Sections::First));
        assert_eq!(config.ignore.unwrap().len(), 2);
        let nav = config.nav.unwrap();
        assert!(matches!(
            &nav[0],
            Item::Named { value: Named::Target(value), .. } if value == "index.md"
        ));
        assert!(matches!(&nav[2], Item::Pattern(_)));
    }

    #[test]
    fn rejects_unknown_and_invalid_options() {
        assert!(parse(".nav.yml", "unknown: true\n").is_err());
        assert!(parse(".nav.yml", "hide: nope\n").is_err());
        assert!(parse(".nav.yml", "nav: [\"\"]\n").is_err());
    }
}
