use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
pub struct SetupMap {
    #[serde(default)]
    pub setup: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_command")]
    pub command: String,
    #[serde(default)]
    pub default_setup: Option<String>,
    #[serde(default)]
    pub languages: SetupMap,
    #[serde(default)]
    pub repos: SetupMap,
}

fn default_command() -> String {
    "claude".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command: default_command(),
            default_setup: None,
            languages: SetupMap::default(),
            repos: SetupMap::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let config: Config =
            serde_yaml::from_str(&contents).with_context(|| "failed to parse tcs.yml")?;
        Ok(config)
    }

    pub fn path() -> PathBuf {
        // Always use ~/.config/tcs.yml (XDG convention), not macOS ~/Library/Application Support/
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("tcs.yml")
    }

    /// Resolve setup command: repo name > language > default
    pub fn setup_command(&self, repo_name: &str, language: Option<&str>) -> Option<String> {
        if let Some(cmd) = self.repos.setup.get(repo_name) {
            return Some(cmd.clone());
        }
        if let Some(lang) = language {
            if let Some(cmd) = self.languages.setup.get(&lang.to_lowercase()) {
                return Some(cmd.clone());
            }
        }
        self.default_setup.clone()
    }
}
