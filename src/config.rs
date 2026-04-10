use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("cmd-helper")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.yaml")
}

pub fn history_db_path() -> PathBuf {
    config_dir().join("history.db")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_model")]
    pub ollama_model: String,
    #[serde(default)]
    pub ollama_api_url: String,
    #[serde(default)]
    pub ollama_api_key: Option<String>,
    #[serde(default = "default_timeout")]
    pub ollama_timeout: u64,
    #[serde(default = "default_output_style")]
    pub output_style: String,
    #[serde(default = "default_true")]
    pub history_enabled: bool,
    #[serde(default = "default_true")]
    pub prompt_actions: bool,
    #[serde(skip)]
    pub history_db_path: PathBuf,
}

fn default_model() -> String {
    if cfg!(target_os = "windows") {
        "command-helper-windows".to_string()
    } else {
        "command-helper".to_string()
    }
}
fn default_timeout() -> u64 {
    10
}
fn default_output_style() -> String {
    "fancy".to_string()
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ollama_model: default_model(),
            ollama_api_url: String::new(),
            ollama_api_key: None,
            ollama_timeout: default_timeout(),
            output_style: default_output_style(),
            history_enabled: true,
            prompt_actions: true,
            history_db_path: history_db_path(),
        }
    }
}

impl Config {
    pub fn load(cli_overrides: HashMap<&str, Option<String>>) -> Self {
        let mut config = Self::default();
        config.apply_file();
        config.apply_env();
        config.apply_overrides(cli_overrides);
        config
    }

    fn apply_file(&mut self) {
        let path = config_file();
        if !path.exists() {
            return;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            return;
        };
        let Ok(data) = serde_yaml::from_str::<Config>(&content) else {
            return;
        };
        // Only override non-default values from file
        if !data.ollama_model.is_empty() {
            self.ollama_model = data.ollama_model;
        }
        if !data.ollama_api_url.is_empty() {
            self.ollama_api_url = data.ollama_api_url;
        }
        if data.ollama_api_key.is_some() {
            self.ollama_api_key = data.ollama_api_key;
        }
        self.ollama_timeout = data.ollama_timeout;
        self.output_style = data.output_style;
        self.history_enabled = data.history_enabled;
        self.prompt_actions = data.prompt_actions;
    }

    fn apply_env(&mut self) {
        if let Ok(val) = env::var("CMD_HELPER_MODEL") {
            self.ollama_model = val;
        }
        if let Ok(val) = env::var("CMD_HELPER_API_URL") {
            self.ollama_api_url = val;
        }
        if let Ok(val) = env::var("CMD_HELPER_API_KEY") {
            self.ollama_api_key = Some(val);
        }
        if let Ok(val) = env::var("CMD_HELPER_TIMEOUT") {
            if let Ok(t) = val.parse() {
                self.ollama_timeout = t;
            }
        }
    }

    fn apply_overrides(&mut self, overrides: HashMap<&str, Option<String>>) {
        if let Some(Some(val)) = overrides.get("ollama_model") {
            self.ollama_model = val.clone();
        }
        if let Some(Some(val)) = overrides.get("ollama_api_url") {
            self.ollama_api_url = val.clone();
        }
        if let Some(Some(val)) = overrides.get("ollama_timeout") {
            if let Ok(t) = val.parse() {
                self.ollama_timeout = t;
            }
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.ollama_api_url.is_empty()
    }

    pub fn save_default_config(&self) -> std::io::Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;

        #[derive(Serialize)]
        struct ConfigFile {
            ollama_model: String,
            ollama_api_url: String,
            ollama_api_key: Option<String>,
            ollama_timeout: u64,
            output_style: String,
            history_enabled: bool,
            prompt_actions: bool,
        }

        let data = ConfigFile {
            ollama_model: self.ollama_model.clone(),
            ollama_api_url: self.ollama_api_url.clone(),
            ollama_api_key: self.ollama_api_key.clone(),
            ollama_timeout: self.ollama_timeout,
            output_style: self.output_style.clone(),
            history_enabled: self.history_enabled,
            prompt_actions: self.prompt_actions,
        };

        let yaml = serde_yaml::to_string(&data).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e)
        })?;
        fs::write(config_file(), yaml)
    }
}
