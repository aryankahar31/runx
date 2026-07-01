use crate::error::UserError;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

pub const CONFIG_FILE: &str = "runx.toml";

#[derive(Debug, Clone, Deserialize)]
pub struct RunxConfig {
    #[serde(default)]
    pub runtimes: BTreeMap<String, String>,
    pub run: BTreeMap<String, String>,
}

impl RunxConfig {
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let path = dir.join(CONFIG_FILE);
        if !path.exists() {
            return Err(UserError::new(format!(
                "No runx.toml found in {}.\nHint: run `runx init` to create a starter config.",
                dir.display()
            ))
            .into());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Self::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
    }

    pub fn from_str(raw: &str) -> Result<Self> {
        let config: Self = toml::from_str(raw)?;
        config.validate()?;
        Ok(config)
    }

    pub fn command(&self, key: &str) -> Result<&str> {
        self.run.get(key).map(String::as_str).ok_or_else(|| {
            let available = self.run.keys().cloned().collect::<Vec<_>>().join(", ");
            UserError::new(format!(
                "No run command named `{key}` found in runx.toml.\nAvailable commands: {available}"
            ))
            .into()
        })
    }

    fn validate(&self) -> Result<()> {
        if self.run.is_empty() {
            return Err(
                UserError::new("runx.toml must contain at least one command under [run].").into(),
            );
        }

        for (tool, version) in &self.runtimes {
            if tool.trim().is_empty() || version.trim().is_empty() {
                return Err(UserError::new("Runtime names and versions cannot be empty.").into());
            }
        }

        for (name, command) in &self.run {
            if name.trim().is_empty() || command.trim().is_empty() {
                return Err(UserError::new("Run command names and values cannot be empty.").into());
            }
        }

        Ok(())
    }
}

pub fn starter_config() -> &'static str {
    r#"[runtimes]
node = "20.11.0"

[run]
dev = "node --version"
build = "node --version"
"#
}

#[cfg(test)]
mod tests {
    use super::RunxConfig;

    #[test]
    fn parses_sample_config() {
        let raw = r#"
[runtimes]
node = "20.11.0"
python = "3.11.7"

[run]
dev = "npm run dev"
build = "npm run build"
"#;

        let config = RunxConfig::from_str(raw).expect("sample config should parse");
        assert_eq!(config.runtimes["node"], "20.11.0");
        assert_eq!(config.runtimes["python"], "3.11.7");
        assert_eq!(config.command("dev").expect("dev command"), "npm run dev");
    }
}
