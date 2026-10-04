//! Canonical label model shared by every layer of `labeldeck`.
//!
//! A [`Label`] is the one representation used in canonical JSON files, in
//! diff/plans, and in requests to GitHub. Colours always carry GitHub's
//! wire representation: six uppercase hexadecimal digits without a leading
//! `#`. Input parsing accepts an optional `#` and any hex case, so files
//! remain human-editable while comparisons stay canonical.

use serde::{Deserialize, Serialize};

/// Maximum length GitHub accepts for a label name.
///
/// GitHub does not document this limit; 50 characters is the observed
/// behaviour (`422 name is too long (maximum is 50 characters)`). The
/// server's 422 remains the authority if the limit ever changes.
pub const MAX_NAME_LEN: usize = 50;

/// Maximum length GitHub accepts for a label description.
pub const MAX_DESCRIPTION_LEN: usize = 100;

/// Number of ASCII hexadecimal digits in a canonical label colour.
pub const LABEL_COLOR_LEN: usize = 6;

/// One canonical label definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Label {
    pub name: String,
    pub color: LabelColor,
    /// Empty string means "no description"; GitHub's `null` maps here.
    pub description: String,
}

/// A validated GitHub label colour: six uppercase hex digits, no `#`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelColor(String);

impl LabelColor {
    /// Parse a colour from file or API input.
    ///
    /// Accepts an optional leading `#` and any ASCII hex case; anything
    /// else (including 3-digit CSS shorthand, which GitHub rejects) is an
    /// error with an actionable message.
    pub fn parse(input: &str) -> Result<Self, String> {
        let digits = input.strip_prefix('#').unwrap_or(input);
        if digits.len() != LABEL_COLOR_LEN {
            return Err(format!(
                "invalid colour {input:?}: expected {LABEL_COLOR_LEN} hexadecimal digits, \
                 found {}",
                digits.len()
            ));
        }
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "invalid colour {input:?}: expected {LABEL_COLOR_LEN} hexadecimal digits"
            ));
        }
        Ok(Self(digits.to_ascii_lowercase()))
    }

    /// The canonical wire/file representation, e.g. `d73a4a` (the form
    /// GitHub itself returns).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LabelColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for LabelColor {
    fn serialize<S: serde::Serializer>(
        &self,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LabelColor {
    fn deserialize<D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        LabelColor::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl Label {
    /// Validate a label that has already been parsed.
    ///
    /// Colour validity is enforced by [`LabelColor::parse`]; this checks
    /// name and description against GitHub's documented limits.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("label name must not be empty".to_string());
        }
        if self.name.chars().count() > MAX_NAME_LEN {
            return Err(format!(
                "label name exceeds GitHub's {}-character limit \
                 ({} characters)",
                MAX_NAME_LEN,
                self.name.chars().count()
            ));
        }
        if self.description.chars().count() > MAX_DESCRIPTION_LEN {
            return Err(format!(
                "description exceeds GitHub's {}-character limit \
                 ({} characters)",
                MAX_DESCRIPTION_LEN,
                self.description.chars().count()
            ));
        }
        Ok(())
    }

    /// Case-folded name used for matching, mirroring GitHub's label-name
    /// uniqueness semantics (names are unique per repository without
    /// regard to case).
    pub fn match_key(&self) -> String {
        self.name.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_parse_accepts_hash_and_case() {
        for input in ["d73a4a", "#d73a4a", "d73a4a", "#D73a4A"] {
            assert_eq!(
                LabelColor::parse(input).unwrap().as_str(),
                "d73a4a",
                "input {input}"
            );
        }
    }

    #[test]
    fn colour_parse_rejects_invalid_values() {
        for input in [
            "",
            "#",
            "d73a4",
            "#d73a4a0",
            "d73a4g",
            "fff",
            "#fff",
            " rgb(1,2,3)",
            "blue",
            "0xFFFFFF",
        ] {
            assert!(LabelColor::parse(input).is_err(), "input {input}");
        }
    }

    #[test]
    fn colour_error_mentions_digit_count() {
        let err = LabelColor::parse("abc").unwrap_err();
        assert!(err.contains("6 hexadecimal digits"), "{err}");
    }

    #[test]
    fn colour_serde_round_trip() {
        let json =
            serde_json::to_string(&LabelColor::parse("#a1b2c3").unwrap())
                .unwrap();
        assert_eq!(json, "\"a1b2c3\"");
        let parsed: LabelColor = serde_json::from_str("\"#a1B2C3\"").unwrap();
        assert_eq!(parsed.as_str(), "a1b2c3");
    }

    #[test]
    fn colour_deserialize_rejects_shorthand() {
        let err = serde_json::from_str::<LabelColor>("\"fff\"").unwrap_err();
        assert!(err.to_string().contains("6 hexadecimal digits"));
    }

    #[test]
    fn validate_rejects_empty_name() {
        let label = Label {
            name: String::new(),
            color: LabelColor::parse("ABC123").unwrap(),
            description: String::new(),
        };
        assert!(label.validate().unwrap_err().contains("empty"));
    }

    #[test]
    fn validate_rejects_long_name() {
        let label = Label {
            name: "x".repeat(51),
            color: LabelColor::parse("abc123").unwrap(),
            description: String::new(),
        };
        assert!(label.validate().unwrap_err().contains("50"));
    }

    #[test]
    fn validate_rejects_long_description() {
        let label = Label {
            name: "bug".to_string(),
            color: LabelColor::parse("ABC123").unwrap(),
            description: "x".repeat(101),
        };
        assert!(label.validate().unwrap_err().contains("description"));
    }

    #[test]
    fn validate_accepts_boundary_lengths() {
        let ok = Label {
            name: "x".repeat(50),
            color: LabelColor::parse("abc123").unwrap(),
            description: "d".repeat(100),
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn match_key_lowercases_for_github_semantics() {
        let label = Label {
            name: "BuG".to_string(),
            color: LabelColor::parse("ABC123").unwrap(),
            description: String::new(),
        };
        assert_eq!(label.match_key(), "bug");
    }
}
