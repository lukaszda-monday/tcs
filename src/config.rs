use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_command")]
    pub command: String,
    #[serde(default)]
    pub default_setup: Option<String>,
    #[serde(default)]
    pub languages: HashMap<String, LangConfig>,
    #[serde(default)]
    pub repos: HashMap<String, RepoConfig>,
}

#[derive(Debug, Deserialize)]
pub struct LangConfig {
    pub setup: String,
}

#[derive(Debug, Deserialize)]
pub struct RepoConfig {
    pub setup: String,
}

fn default_command() -> String {
    "claude".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command: default_command(),
            default_setup: None,
            languages: HashMap::new(),
            repos: HashMap::new(),
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
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("tcs.yml")
    }

    /// Resolve setup command: repo name > language > default
    pub fn setup_command(&self, repo_name: &str, language: Option<&str>) -> Option<String> {
        if let Some(repo_cfg) = self.repos.get(repo_name) {
            return Some(repo_cfg.setup.clone());
        }
        if let Some(lang) = language {
            let lang_lower = lang.to_lowercase();
            if let Some(lang_cfg) = self.languages.get(&lang_lower) {
                return Some(lang_cfg.setup.clone());
            }
        }
        self.default_setup.clone()
    }
}
