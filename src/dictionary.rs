use serde::Deserialize;
use serde::de::{self, Deserializer};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Dictionary {
    pub anchors: Anchors,
    pub dirs: Dirs,
    pub files: Files,
    pub ids: Ids,
    #[serde(default)]
    pub weights: Weights,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Anchors {
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dirs {
    pub common: Vec<String>,
    #[serde(default)]
    pub deep: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Files {
    pub stems: Vec<String>,
    pub extensions: BTreeMap<String, SizeRange>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SizeRange {
    pub min_size: SizeSpec,
    pub max_size: SizeSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeSpec(u64);

impl SizeSpec {
    pub fn value(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SizeSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SizeSpecVisitor;

        impl<'de> de::Visitor<'de> for SizeSpecVisitor {
            type Value = SizeSpec;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a size like 1024, \"1KB\", or \"2MiB\"")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(SizeSpec(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value < 0 {
                    return Err(E::custom("size must be non-negative"));
                }
                Ok(SizeSpec(value as u64))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                parse_size(value).map(SizeSpec).map_err(E::custom)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                parse_size(&value).map(SizeSpec).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(SizeSpecVisitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ids {
    pub formats: Vec<IdFormat>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Weights {
    pub anchors: Option<u32>,
    pub dirs_common: Option<u32>,
    pub dirs_deep: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdFormat {
    Uuid,
    Numeric,
    Date,
    InvoiceCode,
}

impl Dictionary {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("dictionary could not be read: {error}"))?;
        Self::from_toml_str(&contents)
    }

    pub fn from_toml_str(input: &str) -> Result<Self, String> {
        let dictionary: Dictionary = toml::from_str(input)
            .map_err(|error| format!("dictionary is invalid TOML: {error}"))?;
        dictionary.validate()?;
        Ok(dictionary)
    }

    pub fn validate(&self) -> Result<(), String> {
        ensure_non_empty(&self.anchors.roots, "anchors.roots")?;
        ensure_non_empty(&self.dirs.common, "dirs.common")?;
        ensure_non_empty(&self.files.stems, "files.stems")?;
        ensure_extension_ranges(&self.files.extensions)?;
        ensure_non_empty_id_formats(&self.ids.formats)?;
        Ok(())
    }
}

fn ensure_non_empty(values: &[String], field: &str) -> Result<(), String> {
    if values.is_empty() {
        return Err(format!("dictionary requires at least one value in {field}"));
    }

    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(format!("dictionary has empty entries in {field}"));
    }

    Ok(())
}

fn ensure_extension_ranges(values: &BTreeMap<String, SizeRange>) -> Result<(), String> {
    if values.is_empty() {
        return Err("dictionary requires at least one extension in files.extensions".to_string());
    }

    for (extension, range) in values {
        if extension.trim().is_empty() {
            return Err("dictionary has empty extension keys in files.extensions".to_string());
        }

        let normalized = extension.trim_start_matches('.');
        if normalized.is_empty() {
            return Err("dictionary has invalid extension keys in files.extensions".to_string());
        }

        if range.min_size.value() > range.max_size.value() {
            return Err(format!(
                "files.extensions.{extension} has min_size greater than max_size"
            ));
        }
    }

    Ok(())
}

fn parse_size(input: &str) -> Result<u64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("size value cannot be empty".to_string());
    }

    let split_at = trimmed
        .chars()
        .position(|ch| !ch.is_ascii_digit())
        .unwrap_or_else(|| trimmed.len());
    let (value_part, suffix_part) = trimmed.split_at(split_at);
    let value_str = value_part.trim();
    let suffix_str = suffix_part.trim();

    if value_str.is_empty() {
        return Err(format!("invalid size number: {trimmed}"));
    }

    let value: u64 = value_str
        .parse()
        .map_err(|_| format!("invalid size number: {value_str}"))?;

    if suffix_str.is_empty() {
        return Ok(value);
    }

    let suffix = suffix_str
        .replace(char::is_whitespace, "")
        .to_ascii_uppercase();
    let multiplier = match suffix.as_str() {
        "B" => 1,
        "KB" => 1_000,
        "KIB" => 1_024,
        "MB" => 1_000_000,
        "MIB" => 1_048_576,
        _ => {
            return Err(format!("unknown size suffix: {suffix_str}"));
        }
    };

    value
        .checked_mul(multiplier)
        .ok_or_else(|| "size value is too large".to_string())
}

fn ensure_non_empty_id_formats(values: &[IdFormat]) -> Result<(), String> {
    if values.is_empty() {
        return Err("dictionary requires at least one value in ids.formats".to_string());
    }

    Ok(())
}

pub fn default_dictionary() -> Dictionary {
    Dictionary {
        anchors: Anchors {
            roots: vec![
                "etc".to_string(),
                "var".to_string(),
                "srv".to_string(),
                "opt".to_string(),
                "home".to_string(),
                "data".to_string(),
                "logs".to_string(),
                "backups".to_string(),
            ],
        },
        dirs: Dirs {
            common: vec![
                "orders".to_string(),
                "users".to_string(),
                "invoices".to_string(),
                "billing".to_string(),
                "payments".to_string(),
                "exports".to_string(),
                "imports".to_string(),
                "archive".to_string(),
                "reports".to_string(),
                "ledger".to_string(),
            ],
            deep: vec![
                "2026".to_string(),
                "2025".to_string(),
                "2024".to_string(),
                "04".to_string(),
                "05".to_string(),
                "06".to_string(),
                "daily".to_string(),
                "monthly".to_string(),
                "regional".to_string(),
                "batch".to_string(),
                "pending".to_string(),
                "complete".to_string(),
            ],
        },
        files: Files {
            stems: vec![
                "order".to_string(),
                "invoice".to_string(),
                "user".to_string(),
                "receipt".to_string(),
                "export".to_string(),
                "report".to_string(),
                "statement".to_string(),
                "ledger".to_string(),
            ],
            extensions: BTreeMap::from([
                (
                    "json".to_string(),
                    SizeRange {
                        min_size: SizeSpec(256),
                        max_size: SizeSpec(8 * 1024),
                    },
                ),
                (
                    "csv".to_string(),
                    SizeRange {
                        min_size: SizeSpec(256),
                        max_size: SizeSpec(16 * 1024),
                    },
                ),
                (
                    "pdf".to_string(),
                    SizeRange {
                        min_size: SizeSpec(4 * 1024),
                        max_size: SizeSpec(256 * 1024),
                    },
                ),
                (
                    "txt".to_string(),
                    SizeRange {
                        min_size: SizeSpec(128),
                        max_size: SizeSpec(4 * 1024),
                    },
                ),
                (
                    "log".to_string(),
                    SizeRange {
                        min_size: SizeSpec(512),
                        max_size: SizeSpec(64 * 1024),
                    },
                ),
            ]),
        },
        ids: Ids {
            formats: vec![
                IdFormat::Uuid,
                IdFormat::Numeric,
                IdFormat::Date,
                IdFormat::InvoiceCode,
            ],
        },
        weights: Weights {
            anchors: Some(4),
            dirs_common: Some(5),
            dirs_deep: Some(2),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_parses_from_toml() {
        let input = r#"
[anchors]
roots = ["etc", "var"]

[dirs]
common = ["orders", "users"]

[files]
stems = ["order"]
extensions = { json = { min_size = "1KB", max_size = "4KB" } }

[ids]
formats = ["numeric"]
"#;

        let dictionary = Dictionary::from_toml_str(input).expect("dictionary should parse");
        assert_eq!(dictionary.anchors.roots.len(), 2);
        assert!(dictionary.files.extensions.contains_key("json"));
    }

    #[test]
    fn size_parses_with_suffixes() {
        let input = r#"
[anchors]
roots = ["etc"]

[dirs]
common = ["orders"]

[files]
stems = ["order"]
extensions = { json = { min_size = "1KB", max_size = "2MiB" } }

[ids]
formats = ["numeric"]
"#;

        let dictionary = Dictionary::from_toml_str(input).expect("dictionary should parse");
        let range = dictionary.files.extensions.get("json").expect("range");
        assert_eq!(range.min_size.value(), 1_000);
        assert_eq!(range.max_size.value(), 2_097_152);
    }

    #[test]
    fn size_range_requires_min_leq_max() {
        let input = r#"
[anchors]
roots = ["etc"]

[dirs]
common = ["orders"]

[files]
stems = ["order"]
extensions = { json = { min_size = "10KB", max_size = "1KB" } }

[ids]
formats = ["numeric"]
"#;

        let error = Dictionary::from_toml_str(input).expect_err("should fail");
        assert!(error.contains("min_size"));
    }
}
