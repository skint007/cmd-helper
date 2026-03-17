use std::fmt;

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::config::Config;

pub const MODEL_SYSTEM_PROMPT: &str = r#"You are a command-line assistant for Arch Linux. Your job is to translate natural language questions into precise shell commands.

RULES:
1. Always respond with EXACTLY two sections: COMMAND and EXPLANATION. Never add extra sections, alternatives, tips, or summaries.
2. The COMMAND section contains ONLY the shell command(s), nothing else. If multiple commands are needed, separate them with && or use a pipeline.
3. The EXPLANATION section uses bullet points to break down every part of the command:
   - Start each top-level bullet with the command name and a brief description of what it does.
   - Use indented sub-bullets for each flag, option, or argument, explaining what it does.
4. Be precise and concise. Do not include unnecessary flags.
5. All commands must be for Arch Linux. Use pacman, yay, systemctl, journalctl, and other Arch-native tools instead of apt, yum, dnf, or other distro-specific tools.
6. Prefer Arch Linux conventions and paths (e.g., /etc/pacman.conf, makepkg, PKGBUILD).
7. If a question is ambiguous, provide the most common/safe interpretation for Arch Linux.
8. Never include warnings, caveats, or conversational text outside the two sections.
9. Never wrap the command in markdown code fences.
10. Give exactly ONE command solution. Never provide multiple alternatives or methods.

FORMAT (follow exactly):
COMMAND
<the shell command>

EXPLANATION
- <command_name> <what it does>
  - <flag/argument> <what it does>
  - <flag/argument> <what it does>"#;

pub fn model_few_shot_messages() -> Vec<Value> {
    vec![
        json!({"role": "user", "content": "find a file named \"report.txt\" in the current directory"}),
        json!({"role": "assistant", "content": "COMMAND\nfind . -name \"report.txt\"\n\nEXPLANATION\n- find searches for files and directories in a directory hierarchy.\n  - . specifies the current directory as the starting point.\n  - -name \"report.txt\" matches files whose name is exactly report.txt."}),
        json!({"role": "user", "content": "how to list a directory tree 1 level deep"}),
        json!({"role": "assistant", "content": "COMMAND\ntree -L 1\n\nEXPLANATION\n- tree displays the directory structure in a tree-like format.\n  - -L 1 limits the display depth to 1 level, showing only immediate children."}),
    ]
}

#[derive(Debug)]
pub enum OllamaError {
    Connection(String),
    ModelNotFound(String),
    Timeout(String),
    Api(String),
}

impl fmt::Display for OllamaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(msg) => write!(f, "{}", msg),
            Self::ModelNotFound(msg) => write!(f, "{}", msg),
            Self::Timeout(msg) => write!(f, "{}", msg),
            Self::Api(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for OllamaError {}

pub struct OllamaClient {
    config: Config,
    base_url: String,
    http: Client,
}

impl OllamaClient {
    pub fn new(config: &Config) -> Self {
        let base_url = config.ollama_api_url.trim_end_matches('/').to_string();
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(ref key) = config.ollama_api_key {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", key)) {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        let http = Client::builder()
            .default_headers(headers)
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config: config.clone(),
            base_url,
            http,
        }
    }

    pub async fn generate_stream(
        &self,
        prompt: &str,
        system_context: &str,
    ) -> Result<mpsc::Receiver<String>, OllamaError> {
        let full_prompt = if system_context.is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n\n{}", system_context, prompt)
        };

        let payload = json!({
            "model": self.config.ollama_model,
            "prompt": full_prompt,
            "stream": true,
        });

        let url = format!("{}/api/generate", self.base_url);

        let response: reqwest::Response = self
            .http
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e: reqwest::Error| {
                if e.is_connect() || e.is_timeout() {
                    OllamaError::Connection(format!(
                        "Cannot connect to Ollama at {}. Is the server running?\n  Error: {}",
                        self.base_url, e
                    ))
                } else {
                    OllamaError::Api(format!("HTTP error: {}", e))
                }
            })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(OllamaError::ModelNotFound(format!(
                "Model '{}' not found. Run: cmd-helper --create-model",
                self.config.ollama_model
            )));
        }

        if !response.status().is_success() {
            return Err(OllamaError::Api(format!(
                "Ollama API error (HTTP {})",
                response.status()
            )));
        }

        let (tx, rx) = mpsc::channel(64);
        let mut stream = response.bytes_stream();
        let read_timeout = std::time::Duration::from_secs(self.config.ollama_timeout);

        tokio::spawn(async move {
            let mut buffer = String::new();
            loop {
                let chunk_result = tokio::time::timeout(read_timeout, stream.next()).await;
                match chunk_result {
                    Err(_) => break, // read timeout elapsed between chunks
                    Ok(None) => break, // stream ended
                    Ok(Some(Err(_))) => break, // stream error
                    Ok(Some(Ok(ref bytes))) => {
                        buffer.push_str(&String::from_utf8_lossy(bytes));
                    }
                }

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(chunk) = serde_json::from_str::<Value>(trimmed) else {
                        continue;
                    };
                    if let Some(err) = chunk.get("error").and_then(|e| e.as_str()) {
                        if !err.is_empty() {
                            break;
                        }
                    }
                    if let Some(token) = chunk.get("response").and_then(|r| r.as_str()) {
                        if !token.is_empty() && tx.send(token.to_string()).await.is_err() {
                            return;
                        }
                    }
                    if chunk.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }

    pub async fn create_model(&self, model_name: &str) -> Result<mpsc::Receiver<String>, OllamaError> {
        let payload = json!({
            "model": model_name,
            "from": "gpt-oss:20b",
            "system": MODEL_SYSTEM_PROMPT,
            "parameters": {
                "temperature": 0.3,
                "top_p": 0.9,
                "top_k": 40,
                "num_ctx": 4096,
            },
            "messages": model_few_shot_messages(),
            "stream": true,
        });

        let url = format!("{}/api/create", self.base_url);
        let timeout = std::time::Duration::from_secs(300);

        let response: reqwest::Response = self
            .http
            .post(&url)
            .timeout(timeout)
            .json(&payload)
            .send()
            .await
            .map_err(|e: reqwest::Error| {
                if e.is_connect() {
                    OllamaError::Connection(format!(
                        "Cannot connect to Ollama at {}. Is the server running?\n  Error: {}",
                        self.base_url, e
                    ))
                } else if e.is_timeout() {
                    OllamaError::Timeout(
                        "Model creation timed out. The base model may still be downloading. Try again in a few minutes.".to_string()
                    )
                } else {
                    OllamaError::Api(format!("HTTP error: {}", e))
                }
            })?;

        if !response.status().is_success() {
            return Err(OllamaError::Api(format!(
                "Ollama API error (HTTP {})",
                response.status()
            )));
        }

        let (tx, rx) = mpsc::channel(64);
        let mut stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            while let Some(chunk_result) = stream.next().await {
                let Ok(ref bytes) = chunk_result else { break };
                buffer.push_str(&String::from_utf8_lossy(bytes));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(chunk) = serde_json::from_str::<Value>(trimmed) else {
                        continue;
                    };
                    if let Some(err) = chunk.get("error").and_then(|e| e.as_str()) {
                        if !err.is_empty() {
                            break;
                        }
                    }
                    if let Some(status) = chunk.get("status").and_then(|s| s.as_str()) {
                        if !status.is_empty() && tx.send(status.to_string()).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });

        Ok(rx)
    }
}
