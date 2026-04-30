use serde::Deserialize;
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
    pub extensions: Vec<String>,
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
        ensure_non_empty(&self.files.extensions, "files.extensions")?;
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
            extensions: vec![
                "json".to_string(),
                "csv".to_string(),
                "pdf".to_string(),
                "txt".to_string(),
                "log".to_string(),
            ],
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
extensions = ["json"]

[ids]
formats = ["numeric"]
"#;

        let dictionary = Dictionary::from_toml_str(input).expect("dictionary should parse");
        assert_eq!(dictionary.anchors.roots.len(), 2);
        assert_eq!(dictionary.files.extensions[0], "json");
    }
}
