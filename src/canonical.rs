//! Canonical JSON label-set file handling.

use std::collections::HashSet;
use std::fmt::Write as _;

use serde::Deserialize;

use crate::labels::{Label, LabelColor};

/// Serialise labels as the canonical JSON representation.
///
/// The output is deterministic: labels are sorted by name, fields appear
/// in a fixed order, JSON is pretty-printed with two-space indentation,
/// and the text ends with exactly one newline.
pub fn to_json(labels: &mut [Label]) -> String {
    labels.sort_by(|a, b| a.name.cmp(&b.name));
    let mut json = serde_json::to_string_pretty(labels)
        .expect("label serialization cannot fail");
    json.push('\n');
    json
}

/// A label exactly as it may appear in a canonical JSON file.
///
/// Unknown fields are rejected so typos like `colour` fail loudly instead
/// of silently dropping data. `description` may be `null` (or an empty
/// string); both mean "no description".
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLabel {
    name: String,
    color: LabelColor,
    #[serde(deserialize_with = "deserialize_description")]
    description: String,
}

/// Accept `"text"`, `""`, and `null` (meaning no description).
fn deserialize_description<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(d)?;
    Ok(value.unwrap_or_default())
}

/// Failures while reading a canonical label file.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CanonicalError {
    /// The file is not valid JSON.
    #[error("invalid JSON: {0}")]
    Json(String),
    /// A label violates GitHub's field rules.
    #[error("label at index {index}: {reason}")]
    Label { index: usize, reason: String },
    /// Two labels share a name under GitHub's case-insensitive rules.
    #[error(
        "duplicate label name {name:?} at index {index}: GitHub treats \
         label names as unique per repository, ignoring case"
    )]
    Duplicate { index: usize, name: String },
}

/// Parse and validate canonical JSON label-set text.
pub fn parse(text: &str) -> Result<Vec<Label>, CanonicalError> {
    let raw: Vec<RawLabel> = serde_json::from_str(text)
        .map_err(|e| CanonicalError::Json(e.to_string()))?;
    let mut labels = Vec::with_capacity(raw.len());
    let mut seen = HashSet::new();
    for (index, entry) in raw.into_iter().enumerate() {
        let label = Label {
            name: entry.name,
            color: entry.color,
            description: entry.description,
        };
        if let Err(reason) = label.validate() {
            return Err(CanonicalError::Label { index, reason });
        }
        let key = label.match_key();
        if !seen.insert(key) {
            return Err(CanonicalError::Duplicate {
                index,
                name: label.name,
            });
        }
        labels.push(label);
    }
    Ok(labels)
}

/// Render a one-line human explanation for a canonical-file error,
/// prefixed for direct use in CLI diagnostics.
pub fn describe_error(path: &str, error: &CanonicalError) -> String {
    let mut message = String::new();
    let _ = write!(message, "invalid canonical label file {path}: {error}");
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str, color: &str, description: &str) -> Label {
        Label {
            name: name.to_string(),
            color: LabelColor::parse(color).unwrap(),
            description: description.to_string(),
        }
    }

    #[test]
    fn parse_accepts_valid_file() {
        let text = r##"[
  {
    "name": "bug",
    "color": "#d73a4a",
    "description": "Something isn't working"
  },
  {
    "name": "docs",
    "color": "0075ca",
    "description": null
  }
]"##;
        let labels = parse(text).unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].name, "bug");
        assert_eq!(labels[0].color.as_str(), "d73a4a");
        assert_eq!(labels[0].description, "Something isn't working");
        assert_eq!(labels[1].description, "");
    }

    #[test]
    fn parse_accepts_empty_array() {
        assert!(parse("[]").unwrap().is_empty());
    }

    #[test]
    fn parse_rejects_malformed_json() {
        let err = parse("{\"name\":").unwrap_err();
        assert!(err.to_string().contains("invalid JSON"), "{err}");
    }

    #[test]
    fn parse_rejects_non_array_top_level() {
        assert!(parse("{}").is_err());
        assert!(parse("\"labels\"").is_err());
        assert!(parse("42").is_err());
    }

    #[test]
    fn parse_reports_json_position() {
        let err = parse("[\n  {\"name\": \"a\"}\n").unwrap_err();
        let CanonicalError::Json(detail) = err else {
            panic!("expected Json error");
        };
        assert!(detail.contains("line 2"), "{detail}");
    }

    #[test]
    fn parse_rejects_duplicate_names() {
        let text = r#"[
            {"name": "bug", "color": "d73a4a", "description": ""},
            {"name": "BUG", "color": "0075ca", "description": ""}
        ]"#;
        let err = parse(text).unwrap_err();
        assert_eq!(
            err,
            CanonicalError::Duplicate {
                index: 1,
                name: "BUG".to_string()
            }
        );
    }

    #[test]
    fn parse_rejects_invalid_colour() {
        let text =
            r##"[{"name": "bug", "color": "#fff", "description": ""}]"##;
        let err = parse(text).unwrap_err();
        assert!(err.to_string().contains("6 hexadecimal digits"), "{err}");
    }

    #[test]
    fn parse_rejects_empty_name() {
        let text = r#"[{"name": "", "color": "d73a4a", "description": ""}]"#;
        let err = parse(text).unwrap_err();
        assert!(err.to_string().contains("name must not be empty"), "{err}");
    }

    #[test]
    fn parse_rejects_missing_fields() {
        let missing_description = r#"[{"name": "bug", "color": "d73a4a"}]"#;
        assert!(
            parse(missing_description)
                .unwrap_err()
                .to_string()
                .contains("missing field `description`")
        );

        let missing_name = r#"[{"color": "d73a4a", "description": ""}]"#;
        assert!(
            parse(missing_name)
                .unwrap_err()
                .to_string()
                .contains("missing field `name`")
        );
    }

    #[test]
    fn parse_rejects_unknown_fields() {
        let text = r#"[
            {"name": "bug", "color": "d73a4a", "description": "",
             "colour": "d73a4a"}
        ]"#;
        assert!(
            parse(text)
                .unwrap_err()
                .to_string()
                .contains("unknown field `colour`")
        );
    }

    #[test]
    fn to_json_is_deterministic_and_sorted() {
        let mut labels = vec![
            label("zebra", "000000", ""),
            label("ant", "ffffff", "a bug"),
        ];
        let first = to_json(&mut labels);
        let second = to_json(&mut labels);
        assert_eq!(first, second);
        assert!(first.starts_with("[\n  {\n    \"name\": \"ant\""));
        let zebra = first.find("zebra").expect("zebra present");
        let ant = first.find("ant").expect("ant present");
        assert!(ant < zebra, "labels must be sorted by name");
    }

    #[test]
    fn to_json_field_order_and_trailing_newline() {
        let mut labels = vec![label("bug", "d73a4a", "broken")];
        let json = to_json(&mut labels);
        assert_eq!(
            json,
            "[\n  {\n    \"name\": \"bug\",\n    \"color\": \
             \"d73a4a\",\n    \"description\": \"broken\"\n  }\n]\n"
        );
    }

    #[test]
    fn to_json_then_parse_round_trips() {
        let mut labels = vec![
            label("bug", "d73a4a", "Something isn't working"),
            label("docs", "0075ca", ""),
        ];
        let json = to_json(&mut labels);
        let parsed = parse(&json).unwrap();
        labels.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(parsed, labels);
    }
}
