use std::collections::BTreeMap;
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

/// Which API provider to talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Ollama,
    OpenAi,
    Anthropic,
    DeepSeek,
    /// Any OpenAI-compatible endpoint (Groq, OpenRouter, Together, LM Studio, vLLM, ...).
    OpenAiCompatible,
}

/// The wire protocol a provider speaks. Multiple providers can share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKind {
    /// Ollama native `/api/generate` (NDJSON streaming).
    Ollama,
    /// OpenAI Chat Completions `/chat/completions` (SSE streaming).
    OpenAi,
    /// Anthropic Messages `/v1/messages` (SSE streaming).
    Anthropic,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Provider> {
        match s.trim().to_lowercase().replace('_', "-").as_str() {
            "ollama" => Some(Provider::Ollama),
            "openai" => Some(Provider::OpenAi),
            "anthropic" | "claude" => Some(Provider::Anthropic),
            "deepseek" => Some(Provider::DeepSeek),
            "openai-compatible" | "compatible" | "custom" => Some(Provider::OpenAiCompatible),
            _ => None,
        }
    }

    /// The canonical config key for this provider.
    pub fn key(self) -> &'static str {
        match self {
            Provider::Ollama => "ollama",
            Provider::OpenAi => "openai",
            Provider::Anthropic => "anthropic",
            Provider::DeepSeek => "deepseek",
            Provider::OpenAiCompatible => "openai-compatible",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Provider::Ollama => "Ollama",
            Provider::OpenAi => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::DeepSeek => "DeepSeek",
            Provider::OpenAiCompatible => "OpenAI-compatible",
        }
    }

    pub fn api_kind(self) -> ApiKind {
        match self {
            Provider::Ollama => ApiKind::Ollama,
            Provider::Anthropic => ApiKind::Anthropic,
            // OpenAI, DeepSeek and any compatible endpoint all speak chat-completions.
            Provider::OpenAi | Provider::DeepSeek | Provider::OpenAiCompatible => ApiKind::OpenAi,
        }
    }

    /// Default base URL when the user hasn't set one. Empty means "must be provided".
    pub fn default_api_url(self) -> &'static str {
        match self {
            Provider::Ollama => "http://localhost:11434",
            Provider::OpenAi => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::DeepSeek => "https://api.deepseek.com/v1",
            Provider::OpenAiCompatible => "",
        }
    }

    /// Default model when the user hasn't set one. Empty means "must be provided".
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::Ollama => {
                if cfg!(target_os = "windows") {
                    "command-helper-windows"
                } else {
                    "command-helper"
                }
            }
            Provider::OpenAi => "gpt-4o-mini",
            Provider::Anthropic => "claude-opus-4-8",
            Provider::DeepSeek => "deepseek-chat",
            Provider::OpenAiCompatible => "",
        }
    }

    /// Standard environment variable that holds this provider's API key, if any.
    pub fn api_key_env(self) -> Option<&'static str> {
        match self {
            Provider::OpenAi => Some("OPENAI_API_KEY"),
            Provider::Anthropic => Some("ANTHROPIC_API_KEY"),
            Provider::DeepSeek => Some("DEEPSEEK_API_KEY"),
            _ => None,
        }
    }

    /// Whether this provider is unusable without an API key.
    pub fn requires_api_key(self) -> bool {
        matches!(
            self,
            Provider::OpenAi | Provider::Anthropic | Provider::DeepSeek
        )
    }
}

/// Per-provider settings. All fields are optional; unset fields fall back to the
/// provider default (model/url) or to the standard env var (key).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// CLI flag overrides applied on top of file + env config.
#[derive(Debug, Default)]
pub struct Overrides {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Active provider name (raw string; resolve with `provider()`).
    pub provider: String,
    /// Per-provider settings, keyed by provider name.
    pub providers: BTreeMap<String, ProviderConfig>,
    pub timeout: u64,
    pub output_style: String,
    pub history_enabled: bool,
    pub prompt_actions: bool,
    pub history_db_path: PathBuf,
}

fn default_timeout() -> u64 {
    10
}
fn default_output_style() -> String {
    "fancy".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Ollama.key().to_string(),
            providers: BTreeMap::new(),
            timeout: default_timeout(),
            output_style: default_output_style(),
            history_enabled: true,
            prompt_actions: true,
            history_db_path: history_db_path(),
        }
    }
}

/// On-disk shape. Accepts both the new provider-based layout and the legacy
/// flat `ollama_*` keys for backward compatibility.
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    provider: Option<String>,
    #[serde(default)]
    providers: BTreeMap<String, ProviderConfig>,
    timeout: Option<u64>,
    output_style: Option<String>,
    history_enabled: Option<bool>,
    prompt_actions: Option<bool>,
    // Legacy flat keys from older versions (migrated into the ollama provider block).
    ollama_model: Option<String>,
    ollama_api_url: Option<String>,
    ollama_api_key: Option<String>,
    ollama_timeout: Option<u64>,
}

impl Config {
    pub fn load(overrides: Overrides) -> Self {
        let mut config = Self::default();
        config.apply_file();
        config.apply_env();
        config.apply_overrides(overrides);
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
        let Ok(raw) = serde_yaml::from_str::<RawConfig>(&content) else {
            return;
        };

        if let Some(p) = raw.provider.filter(|s| !s.is_empty()) {
            self.provider = p;
        }
        for (k, v) in raw.providers {
            self.providers.insert(k, v);
        }
        // timeout: explicit `timeout` wins, else legacy `ollama_timeout`.
        if let Some(t) = raw.timeout.or(raw.ollama_timeout) {
            self.timeout = t;
        }
        if let Some(s) = raw.output_style.filter(|s| !s.is_empty()) {
            self.output_style = s;
        }
        if let Some(b) = raw.history_enabled {
            self.history_enabled = b;
        }
        if let Some(b) = raw.prompt_actions {
            self.prompt_actions = b;
        }

        // Migrate legacy flat keys into the ollama provider block (without
        // clobbering anything the new-style `providers` map already set).
        if raw.ollama_model.is_some() || raw.ollama_api_url.is_some() || raw.ollama_api_key.is_some()
        {
            let entry = self.providers.entry(Provider::Ollama.key().to_string()).or_default();
            if entry.model.is_none() {
                entry.model = raw.ollama_model.filter(|s| !s.is_empty());
            }
            if entry.api_url.is_none() {
                entry.api_url = raw.ollama_api_url.filter(|s| !s.is_empty());
            }
            if entry.api_key.is_none() {
                entry.api_key = raw.ollama_api_key.filter(|s| !s.is_empty());
            }
        }
    }

    fn apply_env(&mut self) {
        if let Ok(v) = env::var("CMD_HELPER_PROVIDER") {
            if !v.is_empty() {
                self.provider = v;
            }
        }
        // Generic env vars apply to the active provider.
        if let Ok(v) = env::var("CMD_HELPER_MODEL") {
            self.set_active_model(v);
        }
        if let Ok(v) = env::var("CMD_HELPER_API_URL") {
            self.set_active_api_url(v);
        }
        if let Ok(v) = env::var("CMD_HELPER_API_KEY") {
            self.set_active_api_key(v);
        }
        if let Ok(v) = env::var("CMD_HELPER_TIMEOUT") {
            if let Ok(t) = v.parse() {
                self.timeout = t;
            }
        }
    }

    fn apply_overrides(&mut self, o: Overrides) {
        // Provider must be applied first so the field overrides target it.
        if let Some(p) = o.provider.filter(|s| !s.is_empty()) {
            self.provider = p;
        }
        if let Some(m) = o.model {
            self.set_active_model(m);
        }
        if let Some(u) = o.api_url {
            self.set_active_api_url(u);
        }
        if let Some(k) = o.api_key {
            self.set_active_api_key(k);
        }
        if let Some(t) = o.timeout {
            self.timeout = t;
        }
    }

    fn active_entry_mut(&mut self) -> &mut ProviderConfig {
        self.providers.entry(self.provider.clone()).or_default()
    }
    fn set_active_model(&mut self, v: String) {
        if !v.is_empty() {
            self.active_entry_mut().model = Some(v);
        }
    }
    fn set_active_api_url(&mut self, v: String) {
        if !v.is_empty() {
            self.active_entry_mut().api_url = Some(v);
        }
    }
    fn set_active_api_key(&mut self, v: String) {
        if !v.is_empty() {
            self.active_entry_mut().api_key = Some(v);
        }
    }

    /// Resolve the active provider, or `None` if the name is unknown.
    pub fn provider(&self) -> Option<Provider> {
        Provider::parse(&self.provider)
    }

    fn active_config(&self) -> Option<&ProviderConfig> {
        self.providers.get(&self.provider)
    }

    /// Resolved model: explicit config, else provider default.
    pub fn active_model(&self) -> String {
        let explicit = self
            .active_config()
            .and_then(|c| c.model.clone())
            .filter(|s| !s.is_empty());
        explicit.unwrap_or_else(|| {
            self.provider()
                .map(|p| p.default_model().to_string())
                .unwrap_or_default()
        })
    }

    /// Resolved base URL: explicit config, else provider default.
    pub fn active_api_url(&self) -> String {
        let explicit = self
            .active_config()
            .and_then(|c| c.api_url.clone())
            .filter(|s| !s.is_empty());
        explicit.unwrap_or_else(|| {
            self.provider()
                .map(|p| p.default_api_url().to_string())
                .unwrap_or_default()
        })
    }

    /// Resolved API key: explicit config, else the provider's standard env var.
    pub fn active_api_key(&self) -> Option<String> {
        let explicit = self
            .active_config()
            .and_then(|c| c.api_key.clone())
            .filter(|s| !s.is_empty());
        explicit.or_else(|| {
            self.provider()
                .and_then(|p| p.api_key_env())
                .and_then(|e| env::var(e).ok())
                .filter(|s| !s.is_empty())
        })
    }

    /// Whether the active provider has everything it needs to make a request.
    pub fn is_configured(&self) -> bool {
        let Some(provider) = self.provider() else {
            return false;
        };
        if self.active_api_url().is_empty() {
            return false;
        }
        if provider.requires_api_key() && self.active_api_key().is_none() {
            return false;
        }
        true
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;

        #[derive(Serialize)]
        struct FileConfig<'a> {
            provider: &'a str,
            providers: &'a BTreeMap<String, ProviderConfig>,
            timeout: u64,
            output_style: &'a str,
            history_enabled: bool,
            prompt_actions: bool,
        }

        let data = FileConfig {
            provider: &self.provider,
            providers: &self.providers,
            timeout: self.timeout,
            output_style: &self.output_style,
            history_enabled: self.history_enabled,
            prompt_actions: self.prompt_actions,
        };

        let yaml = serde_yaml::to_string(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(config_file(), yaml)
    }
}
