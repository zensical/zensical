// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Source-aware YAML parsing for metadata.

use anyhow::{bail, Result};
use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};
use std::collections::BTreeMap;

use super::{Document, Node, Origin, SourceSpan, Value};
use crate::structure::dynamic::Dynamic;

/// Parses one YAML mapping and retains source ranges on every value.
pub(super) fn parse(
    path: &str, source: &str, offset: usize,
) -> Result<Document> {
    let (source, offset) = source
        .strip_prefix('\u{FEFF}')
        .map_or((source, offset), |source| (source, offset + 3));
    let documents = MarkedYaml::load_from_str(source)?;
    if documents.len() != 1 {
        bail!("metadata must contain exactly one YAML document")
    }
    let root = convert(path, source, offset, &documents[0])?;
    match root.value {
        Value::Map(_) => Ok(Document { path: path.into(), root }),
        Value::Scalar(Dynamic::Null) if source.trim().is_empty() => {
            Ok(Document {
                path: path.into(),
                root: Node {
                    origin: root.origin,
                    value: Value::Map(BTreeMap::new()),
                },
            })
        }
        _ => bail!("metadata root must be a mapping"),
    }
}

/// Extracts front matter using the same delimiters as Python Markdown.
pub(super) fn front_matter(
    path: &str, source: &str,
) -> Result<(String, Option<Document>)> {
    let (source, source_offset) = source
        .strip_prefix('\u{FEFF}')
        .map_or((source, 0), |source| (source, 3));
    let mut lines = source.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Ok((source.into(), None));
    };
    if !is_delimiter(first, "---") {
        return Ok((source.into(), None));
    }

    let yaml_start = first.len();
    let mut cursor = yaml_start;
    for line in lines {
        if is_delimiter(line, "---") || is_delimiter(line, "...") {
            let yaml = &source[yaml_start..cursor];
            let body = source[cursor + line.len()..]
                .trim_start_matches('\n')
                .to_owned();
            return parse(path, yaml, yaml_start + source_offset)
                .map(|document| (body, Some(document)));
        }
        cursor += line.len();
    }
    Ok((source.into(), None))
}

/// Returns whether a complete line is a front-matter delimiter.
fn is_delimiter(line: &str, delimiter: &str) -> bool {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    line.strip_prefix(delimiter)
        .is_some_and(|suffix| suffix.chars().all(|ch| matches!(ch, ' ' | '\t')))
}

/// Converts one Saphyr node into an owned source-aware node.
fn convert(
    path: &str, source: &str, offset: usize, node: &MarkedYaml<'_>,
) -> Result<Node> {
    let origin = Origin::Source(SourceSpan {
        source: path.into(),
        range: marker_to_byte(source, node.span.start.index()) + offset
            ..marker_to_byte(source, node.span.end.index()) + offset,
    });
    let value = match &node.data {
        YamlData::Value(value) => Value::Scalar(convert_scalar(value)),
        YamlData::Sequence(values) => Value::List(
            values
                .iter()
                .map(|value| convert(path, source, offset, value))
                .collect::<Result<_>>()?,
        ),
        YamlData::Mapping(values) => {
            let mut explicit = BTreeMap::new();
            let mut inherited = BTreeMap::new();
            for (key, value) in values {
                let key = string_key(key)?;
                let value = convert(path, source, offset, value)?;
                if key == "<<" {
                    merge_key(&mut inherited, value)?;
                } else {
                    explicit.insert(key, value);
                }
            }
            inherited.extend(explicit);
            Value::Map(inherited)
        }
        YamlData::Tagged(_, _) => bail!("custom YAML tags are not supported"),
        YamlData::Alias(_) => bail!("unresolved YAML alias"),
        YamlData::BadValue => bail!("invalid YAML value"),
        YamlData::Representation(_, _, _) => {
            unreachable!("Saphyr performs early scalar parsing")
        }
    };
    Ok(Node { origin, value })
}

/// Converts a Saphyr scalar to the common metadata representation.
fn convert_scalar(value: &Scalar<'_>) -> Dynamic {
    match value {
        Scalar::Null => Dynamic::Null,
        Scalar::Boolean(value) => Dynamic::Bool(*value),
        Scalar::Integer(value) => Dynamic::Integer(*value),
        Scalar::FloatingPoint(value) => Dynamic::from_float(value.into_inner()),
        Scalar::String(value) => Dynamic::String(value.to_string()),
    }
}

/// Requires mapping keys to be strings.
fn string_key(node: &MarkedYaml<'_>) -> Result<String> {
    match &node.data {
        YamlData::Value(Scalar::String(value)) => Ok(value.to_string()),
        _ => bail!("metadata mapping keys must be strings"),
    }
}

/// Expands the YAML merge key while retaining the referenced node origins.
fn merge_key(target: &mut BTreeMap<String, Node>, node: Node) -> Result<()> {
    match node.value {
        Value::Map(values) => {
            for (key, value) in values {
                target.entry(key).or_insert(value);
            }
            Ok(())
        }
        Value::List(values) => {
            for value in values {
                let Value::Map(values) = value.value else {
                    bail!("YAML merge sequence entries must be mappings")
                };
                for (key, value) in values {
                    target.entry(key).or_insert(value);
                }
            }
            Ok(())
        }
        Value::Scalar(_) => {
            bail!("YAML merge value must be a mapping or list of mappings")
        }
    }
}

/// Converts Saphyr's character index to a UTF-8 byte offset.
fn marker_to_byte(source: &str, index: usize) -> usize {
    if index == source.chars().count() {
        source.len()
    } else {
        source
            .char_indices()
            .nth(index)
            .map_or(source.len(), |(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_unicode_byte_ranges() {
        let document = parse("docs/.meta.yml", "title: Héllo\n", 0).unwrap();
        let Value::Map(values) = document.root.value else {
            panic!("mapping")
        };
        let Origin::Source(span) = &values["title"].origin else {
            panic!("source")
        };
        assert_eq!(&"title: Héllo\n"[span.range.clone()], "Héllo");
    }

    #[test]
    fn extracts_front_matter_and_offsets_ranges() {
        let source = "---\ntitle: Home\n---\n\n# Home\n";
        let (body, document) = front_matter("docs/index.md", source).unwrap();
        assert_eq!(body, "# Home\n");
        let document = document.unwrap();
        let Value::Map(values) = document.root.value else {
            panic!("mapping")
        };
        let Origin::Source(span) = &values["title"].origin else {
            panic!("source")
        };
        assert_eq!(&source[span.range.clone()], "Home");
    }

    #[test]
    fn expands_alias_merge_keys() {
        let source = "base: &base\n  one: 1\nvalue:\n  <<: *base\n  two: 2\n";
        let document = parse("docs/.meta.yml", source, 0).unwrap();
        let Value::Map(root) = document.root.value else {
            panic!("mapping")
        };
        let Value::Map(value) = &root["value"].value else {
            panic!("mapping")
        };
        assert!(value.contains_key("one"));
        assert!(value.contains_key("two"));
    }
}
