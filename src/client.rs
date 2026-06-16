use std::fmt;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::config::{ApiKind, Config, Provider};

const LINUX_SYSTEM_PROMPT: &str = r#"You are a command-line assistant for Arch Linux. Your job is to translate natural language questions into precise shell commands.

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

const WINDOWS_SYSTEM_PROMPT: &str = r#"You are a command-line assistant for Windows. Your job is to translate natural language questions into precise PowerShell commands.

RULES:
1. Always respond with EXACTLY two sections: COMMAND and EXPLANATION. Never add extra sections, alternatives, tips, or summaries.
2. The COMMAND section contains ONLY the PowerShell command(s), nothing else. If multiple commands are needed, separate them with ; or use a pipeline.
3. The EXPLANATION section uses bullet points to break down every part of the command:
   - Start each top-level bullet with the cmdlet name and a brief description of what it does.
   - Use indented sub-bullets for each parameter or argument, explaining what it does.
4. Be precise and concise. Do not include unnecessary parameters.
5. All commands must be PowerShell. Use native PowerShell cmdlets (Get-ChildItem, Get-Process, etc.) instead of legacy cmd.exe commands when a PowerShell equivalent exists.
6. Prefer PowerShell conventions and standard Windows paths (e.g., $env:USERPROFILE, $env:APPDATA, C:\).
7. If a question is ambiguous, provide the most common/safe interpretation for Windows.
8. Never include warnings, caveats, or conversational text outside the two sections.
9. Never wrap the command in markdown code fences.
10. Give exactly ONE command solution. Never provide multiple alternatives or methods.

FORMAT (follow exactly):
COMMAND
<the PowerShell command>

EXPLANATION
- <cmdlet_name> <what it does>
  - <parameter> <what it does>
  - <parameter> <what it does>"#;

pub fn model_system_prompt() -> &'static str {
    if cfg!(target_os = "windows") {
        WINDOWS_SYSTEM_PROMPT
    } else {
        LINUX_SYSTEM_PROMPT
    }
}

fn linux_few_shot_messages() -> Vec<Value> {
    vec![
        json!({"role": "user", "content": "find a file named \"report.txt\" in the current directory"}),
        json!({"role": "assistant", "content": "COMMAND\nfind . -name \"report.txt\"\n\nEXPLANATION\n- find searches for files and directories in a directory hierarchy.\n  - . specifies the current directory as the starting point.\n  - -name \"report.txt\" matches files whose name is exactly report.txt."}),
        json!({"role": "user", "content": "how to list a directory tree 1 level deep"}),
        json!({"role": "assistant", "content": "COMMAND\ntree -L 1\n\nEXPLANATION\n- tree displays the directory structure in a tree-like format.\n  - -L 1 limits the display depth to 1 level, showing only immediate children."}),
    ]
}

fn windows_few_shot_messages() -> Vec<Value> {
    vec![
        json!({"role": "user", "content": "find a file named \"report.txt\" in the current directory"}),
        json!({"role": "assistant", "content": "COMMAND\nGet-ChildItem -Recurse -Filter \"report.txt\"\n\nEXPLANATION\n- Get-ChildItem lists items in a directory.\n  - -Recurse searches all subdirectories.\n  - -Filter \"report.txt\" matches files whose name is exactly report.txt."}),
        json!({"role": "user", "content": "how to list a directory tree 1 level deep"}),
        json!({"role": "assistant", "content": "COMMAND\nGet-ChildItem | Format-Table Name, Mode\n\nEXPLANATION\n- Get-ChildItem lists items in the current directory (one level by default).\n- Format-Table displays the output as a table.\n  - Name, Mode shows the file/folder name and its attributes."}),
    ]
}

pub fn model_few_shot_messages() -> Vec<Value> {
    if cfg!(target_os = "windows") {
        windows_few_shot_messages()
    } else {
        linux_few_shot_messages()
    }
}

#[derive(Debug)]
pub enum ClientError {
    Connection(String),
    ModelNotFound(String),
    Timeout(String),
    Auth(String),
    Api(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(msg) => write!(f, "{}", msg),
            Self::ModelNotFound(msg) => write!(f, "{}", msg),
            Self::Timeout(msg) => write!(f, "{}", msg),
            Self::Auth(msg) => write!(f, "{}", msg),
            Self::Api(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ClientError {}

pub struct LlmClient {
    provider: Provider,
    model: String,
    base_url: String,
    timeout: u64,
    http: Client,
}

impl LlmClient {
    pub fn new(config: &Config) -> Result<Self, ClientError> {
        let provider = config.provider().ok_or_else(|| {
            ClientError::Api(format!(
                "Unknown provider '{}'. Valid providers: ollama, openai, anthropic, deepseek, openai-compatible",
                config.provider
            ))
        })?;

        let model = config.active_model();
        let base_url = config.active_api_url().trim_end_matches('/').to_string();
        let api_key = config.active_api_key();

        let mut headers = reqwest::header::HeaderMap::new();
        match provider.api_kind() {
            ApiKind::Ollama | ApiKind::OpenAi => {
                if let Some(ref key) = api_key {
                    if let Ok(val) =
                        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", key))
                    {
                        headers.insert(reqwest::header::AUTHORIZATION, val);
                    }
                }
            }
            ApiKind::Anthropic => {
                if let Some(ref key) = api_key {
                    if let Ok(val) = reqwest::header::HeaderValue::from_str(key) {
                        headers.insert("x-api-key", val);
                    }
                }
                headers.insert(
                    "anthropic-version",
                    reqwest::header::HeaderValue::from_static("2023-06-01"),
                );
            }
        }

        let http = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| ClientError::Api(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            provider,
            model,
            base_url,
            timeout: config.timeout,
            http,
        })
    }

    /// Build the request URL and JSON body for the active provider.
    fn build_request(&self, prompt: &str, system_context: &str) -> (String, Value) {
        let user_content = if system_context.is_empty() {
            prompt.to_string()
        } else {
            format!("{}\n\n{}", system_context, prompt)
        };

        match self.provider.api_kind() {
            ApiKind::Ollama => {
                let url = format!("{}/api/generate", self.base_url);
                let payload = json!({
                    "model": self.model,
                    "prompt": user_content,
                    "stream": true,
                });
                (url, payload)
            }
            ApiKind::OpenAi => {
                let url = format!("{}/chat/completions", self.base_url);
                let mut messages = vec![json!({"role": "system", "content": model_system_prompt()})];
                messages.extend(model_few_shot_messages());
                messages.push(json!({"role": "user", "content": user_content}));
                let payload = json!({
                    "model": self.model,
                    "messages": messages,
                    "stream": true,
                });
                (url, payload)
            }
            ApiKind::Anthropic => {
                let url = format!("{}/v1/messages", self.base_url);
                let mut messages = model_few_shot_messages();
                messages.push(json!({"role": "user", "content": user_content}));
                let payload = json!({
                    "model": self.model,
                    "max_tokens": 2048,
                    "system": model_system_prompt(),
                    "messages": messages,
                    "stream": true,
                });
                (url, payload)
            }
        }
    }

    pub async fn generate_stream(
        &self,
        prompt: &str,
        system_context: &str,
    ) -> Result<mpsc::Receiver<String>, ClientError> {
        let kind = self.provider.api_kind();
        let (url, payload) = self.build_request(prompt, system_context);

        let response: reqwest::Response = self
            .http
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e: reqwest::Error| {
                if e.is_connect() || e.is_timeout() {
                    let hint = if kind == ApiKind::Ollama {
                        "Is the server running?"
                    } else {
                        "Check the URL and your network connection."
                    };
                    ClientError::Connection(format!(
                        "Cannot connect to {} at {}. {}\n  Error: {}",
                        self.provider.display_name(),
                        self.base_url,
                        hint,
                        e
                    ))
                } else {
                    ClientError::Api(format!("HTTP error: {}", e))
                }
            })?;

        let status = response.status();

        if status == reqwest::StatusCode::NOT_FOUND {
            if kind == ApiKind::Ollama {
                return Err(ClientError::ModelNotFound(format!(
                    "Model '{}' not found. Run: cmd-helper --create-model",
                    self.model
                )));
            }
            let body = response.text().await.unwrap_or_default();
            return Err(ClientError::ModelNotFound(format!(
                "{} model '{}' not found (HTTP 404).{}",
                self.provider.display_name(),
                self.model,
                detail(&body)
            )));
        }

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            let body = response.text().await.unwrap_or_default();
            return Err(ClientError::Auth(format!(
                "{} authentication failed (HTTP {}). Check your API key.{}",
                self.provider.display_name(),
                status.as_u16(),
                detail(&body)
            )));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ClientError::Api(format!(
                "{} API error (HTTP {}){}",
                self.provider.display_name(),
                status.as_u16(),
                detail(&body)
            )));
        }

        let (tx, rx) = mpsc::channel(64);
        let mut stream = response.bytes_stream();
        let read_timeout = Duration::from_secs(self.timeout);

        tokio::spawn(async move {
            let mut buffer = String::new();
            loop {
                let chunk_result = tokio::time::timeout(read_timeout, stream.next()).await;
                match chunk_result {
                    Err(_) => break,         // read timeout elapsed between chunks
                    Ok(None) => break,       // stream ended
                    Ok(Some(Err(_))) => break, // stream error
                    Ok(Some(Ok(ref bytes))) => {
                        buffer.push_str(&String::from_utf8_lossy(bytes));
                    }
                }

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].to_string();
                    buffer.replace_range(..=pos, "");

                    let result = parse_stream_line(kind, &line);
                    if let Some(token) = result.token {
                        if !token.is_empty() && tx.send(token).await.is_err() {
                            return;
                        }
                    }
                    if result.done {
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Create/update the command-helper model on an Ollama server.
    /// Only supported for the Ollama provider.
    pub async fn create_model(
        &self,
        model_name: &str,
        base_model: &str,
    ) -> Result<mpsc::Receiver<String>, ClientError> {
        if self.provider != Provider::Ollama {
            return Err(ClientError::Api(
                "--create-model is only supported for the Ollama provider.".to_string(),
            ));
        }

        let payload = json!({
            "model": model_name,
            "from": base_model,
            "system": model_system_prompt(),
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
        let timeout = Duration::from_secs(300);

        let response: reqwest::Response = self
            .http
            .post(&url)
            .timeout(timeout)
            .json(&payload)
            .send()
            .await
            .map_err(|e: reqwest::Error| {
                if e.is_connect() {
                    ClientError::Connection(format!(
                        "Cannot connect to Ollama at {}. Is the server running?\n  Error: {}",
                        self.base_url, e
                    ))
                } else if e.is_timeout() {
                    ClientError::Timeout(
                        "Model creation timed out. The base model may still be downloading. Try again in a few minutes.".to_string()
                    )
                } else {
                    ClientError::Api(format!("HTTP error: {}", e))
                }
            })?;

        if !response.status().is_success() {
            return Err(ClientError::Api(format!(
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

struct LineResult {
    token: Option<String>,
    done: bool,
}

/// Parse one line of a streaming response into an optional token and a done flag.
/// Handles Ollama NDJSON, OpenAI SSE, and Anthropic SSE.
fn parse_stream_line(kind: ApiKind, line: &str) -> LineResult {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return LineResult { token: None, done: false };
    }

    match kind {
        ApiKind::Ollama => {
            let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
                return LineResult { token: None, done: false };
            };
            let errored = v
                .get("error")
                .and_then(|e| e.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            let token = v
                .get("response")
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());
            let done = errored || v.get("done").and_then(|d| d.as_bool()).unwrap_or(false);
            LineResult { token, done }
        }
        ApiKind::OpenAi => {
            // SSE: only `data:` lines carry payloads.
            let Some(data) = trimmed.strip_prefix("data:") else {
                return LineResult { token: None, done: false };
            };
            let data = data.trim();
            if data == "[DONE]" {
                return LineResult { token: None, done: true };
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                return LineResult { token: None, done: false };
            };
            let token = v
                .pointer("/choices/0/delta/content")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string());
            LineResult { token, done: false }
        }
        ApiKind::Anthropic => {
            let Some(data) = trimmed.strip_prefix("data:") else {
                return LineResult { token: None, done: false };
            };
            let data = data.trim();
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                return LineResult { token: None, done: false };
            };
            match v.get("type").and_then(|t| t.as_str()) {
                Some("content_block_delta") => {
                    let token = v
                        .pointer("/delta/text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string());
                    LineResult { token, done: false }
                }
                Some("message_stop") | Some("error") => LineResult { token: None, done: true },
                _ => LineResult { token: None, done: false },
            }
        }
    }
}

/// Extract a short, human-readable detail from an error response body.
fn detail(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Try common JSON error shapes: {"error": {"message": ...}} or {"error": "..."}.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .or_else(|| v.get("error").and_then(|e| e.as_str()))
            .or_else(|| v.get("message").and_then(|m| m.as_str()));
        if let Some(msg) = msg {
            return format!("\n  {}", msg);
        }
    }
    let snippet: String = trimmed.chars().take(200).collect();
    format!("\n  {}", snippet)
}
