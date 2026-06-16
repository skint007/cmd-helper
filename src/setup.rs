use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::config::{Config, Provider, ProviderConfig, config_file};
use crate::display::{print_dim, print_success, print_warning};

const ALIAS_BEGIN: &str = "# BEGIN cmd-helper";
const ALIAS_END: &str = "# END cmd-helper";
const ALIAS_BASH_ZSH: &str = "alias '??'='cmd-helper'";
const ALIAS_FISH: &str = "alias '??' 'cmd-helper'";
const ALIAS_POWERSHELL: &str = "Set-Alias -Name '??' -Value 'cmd-helper'";

fn detect_shell() -> String {
    if let Ok(shell) = env::var("SHELL") {
        let name = Path::new(&shell)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if matches!(name.as_str(), "bash" | "zsh" | "fish") {
            return name;
        }
    }
    if cfg!(target_os = "windows") {
        return "powershell".to_string();
    }
    "bash".to_string()
}

fn detect_powershell_profile() -> Option<PathBuf> {
    // Try pwsh (PowerShell 7+) first, then powershell (Windows PowerShell 5.1)
    for cmd in &["pwsh", "powershell"] {
        if let Ok(output) = Command::new(cmd)
            .args(["-NoProfile", "-Command", "echo $PROFILE"])
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    None
}

fn get_rc_file(shell: &str) -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    match shell {
        "zsh" => home.join(".zshrc"),
        "fish" => home.join(".config").join("fish").join("config.fish"),
        "powershell" => detect_powershell_profile().unwrap_or_else(|| {
            home.join("Documents")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1")
        }),
        _ => home.join(".bashrc"),
    }
}

fn alias_line(shell: &str) -> &'static str {
    match shell {
        "fish" => ALIAS_FISH,
        "powershell" => ALIAS_POWERSHELL,
        _ => ALIAS_BASH_ZSH,
    }
}

pub fn install_alias(shell: Option<&str>) -> bool {
    let shell = shell
        .map(|s| s.to_string())
        .unwrap_or_else(detect_shell);
    let rc_file = get_rc_file(&shell);
    let alias = alias_line(&shell);
    let block = format!("\n{}\n{}\n{}\n", ALIAS_BEGIN, alias, ALIAS_END);

    if rc_file.exists() {
        if let Ok(content) = fs::read_to_string(&rc_file) {
            if content.contains(ALIAS_BEGIN) {
                return false;
            }
        }
    }

    if let Some(parent) = rc_file.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&rc_file)
        .expect("Cannot open shell RC file");
    let _ = f.write_all(block.as_bytes());
    true
}

pub fn remove_alias(shell: Option<&str>) -> bool {
    let shell = shell
        .map(|s| s.to_string())
        .unwrap_or_else(detect_shell);
    let rc_file = get_rc_file(&shell);

    if !rc_file.exists() {
        return false;
    }

    let Ok(content) = fs::read_to_string(&rc_file) else {
        return false;
    };

    if !content.contains(ALIAS_BEGIN) {
        return false;
    }

    let pattern = format!(
        r"\n?{}\n.*?\n{}\n?",
        regex::escape(ALIAS_BEGIN),
        regex::escape(ALIAS_END)
    );
    let re = Regex::new(&pattern).unwrap();
    let new_content = re.replace(&content, "\n").to_string();
    let _ = fs::write(&rc_file, new_content);
    true
}

fn prompt_input(prompt: &str) -> String {
    print!("{}", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    input.trim().to_string()
}

fn mask_key(key: &str) -> String {
    format!("{}****", &key[..key.len().min(4)])
}

/// Prompt for the active provider, returning the chosen provider key.
fn prompt_provider(current: &str) -> String {
    println!("\x1b[1mProvider:\x1b[0m");
    println!("  1. Ollama");
    println!("  2. OpenAI");
    println!("  3. Anthropic");
    println!("  4. DeepSeek");
    println!("  5. OpenAI-compatible (Groq, OpenRouter, LM Studio, ...)");
    let current_label = Provider::parse(current)
        .map(|p| p.key())
        .unwrap_or(current);
    loop {
        let input = prompt_input(&format!(
            "\x1b[1mSelect provider [{}]:\x1b[0m ",
            current_label
        ));
        if input.is_empty() {
            return current.to_string();
        }
        let key = match input.as_str() {
            "1" => Some("ollama"),
            "2" => Some("openai"),
            "3" => Some("anthropic"),
            "4" => Some("deepseek"),
            "5" => Some("openai-compatible"),
            other => Provider::parse(other).map(|p| p.key()),
        };
        match key {
            Some(k) => return k.to_string(),
            None => print_warning("Invalid provider. Enter a number 1-5 or a name."),
        }
    }
}

fn prompt_model(pc: &mut ProviderConfig, default: &str, required: bool) {
    let current = pc.model.clone().unwrap_or_default();
    let shown = if current.is_empty() {
        default.to_string()
    } else {
        current.clone()
    };
    loop {
        let hint = if shown.is_empty() {
            String::new()
        } else {
            format!(" [{}]", shown)
        };
        let v = prompt_input(&format!("\x1b[1mModel name{}:\x1b[0m ", hint));
        if !v.is_empty() {
            pc.model = Some(v);
            return;
        }
        if !shown.is_empty() {
            pc.model = Some(shown);
            return;
        }
        if !required {
            return;
        }
        print_warning("Model is required.");
    }
}

/// Prompt for a base URL. `required` rejects empty input when no value exists.
/// `store_default` controls whether accepting the default writes it into config
/// (true for Ollama's localhost; false for cloud providers so only the key is saved).
fn prompt_url(pc: &mut ProviderConfig, label: &str, default: &str, required: bool, store_default: bool) {
    let current = pc.api_url.clone().unwrap_or_default();
    let shown = if current.is_empty() {
        default.to_string()
    } else {
        current.clone()
    };
    loop {
        let hint = if shown.is_empty() {
            String::new()
        } else {
            format!(" [{}]", shown)
        };
        let v = prompt_input(&format!("\x1b[1m{}{}:\x1b[0m ", label, hint));
        if !v.is_empty() {
            pc.api_url = Some(v);
            return;
        }
        if !current.is_empty() {
            return; // keep existing value
        }
        if !default.is_empty() {
            if store_default {
                pc.api_url = Some(default.to_string());
            }
            return; // accept default (left unset for cloud)
        }
        if !required {
            return;
        }
        print_warning("URL is required.");
    }
}

fn prompt_optional_key(pc: &mut ProviderConfig) {
    let prompt = match &pc.api_key {
        Some(key) => format!("\x1b[1mAPI key [{}]:\x1b[0m ", mask_key(key)),
        None => "\x1b[1mAPI key (Enter to skip):\x1b[0m ".to_string(),
    };
    let v = prompt_input(&prompt);
    if !v.is_empty() {
        pc.api_key = Some(v);
    }
}

fn prompt_required_key(pc: &mut ProviderConfig, provider: Provider) {
    loop {
        let prompt = match &pc.api_key {
            Some(key) => format!("\x1b[1mAPI key [{}]:\x1b[0m ", mask_key(key)),
            None => "\x1b[1mAPI key:\x1b[0m ".to_string(),
        };
        let v = prompt_input(&prompt);
        if !v.is_empty() {
            pc.api_key = Some(v);
            return;
        }
        if pc.api_key.is_some() {
            return; // keep existing key
        }
        // Allow skipping if the standard env var is set.
        if let Some(name) = provider.api_key_env() {
            if env::var(name).map(|v| !v.is_empty()).unwrap_or(false) {
                print_dim(&format!("Using {} from the environment.", name));
                return;
            }
        }
        print_warning("API key is required.");
    }
}

pub fn interactive_setup(config: &mut Config) {
    println!();
    println!("\x1b[1;36mcmd-helper setup\x1b[0m");
    println!("\x1b[2mConfigure your command-line AI assistant.\x1b[0m");
    println!();

    // Provider selection
    config.provider = prompt_provider(&config.provider);
    let provider = Provider::parse(&config.provider).unwrap_or(Provider::Ollama);
    println!();

    // Per-provider settings (work on a copy, then store it back).
    let mut pc = config.providers.get(&config.provider).cloned().unwrap_or_default();

    match provider {
        Provider::Ollama => {
            prompt_url(&mut pc, "Ollama API URL", provider.default_api_url(), true, true);
            prompt_model(&mut pc, provider.default_model(), true);
            prompt_optional_key(&mut pc);
        }
        Provider::OpenAiCompatible => {
            prompt_url(&mut pc, "API base URL", provider.default_api_url(), true, true);
            prompt_model(&mut pc, provider.default_model(), true);
            prompt_optional_key(&mut pc);
        }
        // OpenAI / Anthropic / DeepSeek: key required, sensible URL + model defaults.
        _ => {
            prompt_required_key(&mut pc, provider);
            prompt_model(&mut pc, provider.default_model(), true);
            prompt_url(&mut pc, "API base URL", provider.default_api_url(), false, false);
        }
    }

    config.providers.insert(config.provider.clone(), pc);
    println!();

    // Timeout (shared across providers)
    let timeout_str = prompt_input(&format!(
        "\x1b[1mRequest timeout in seconds [{}]:\x1b[0m ",
        config.timeout
    ));
    if !timeout_str.is_empty() {
        if let Ok(t) = timeout_str.parse() {
            config.timeout = t;
        } else {
            print_warning("Invalid number, keeping default.");
        }
    }

    // Output style
    let style = prompt_input(&format!(
        "\x1b[1mOutput style - fancy or simple [{}]:\x1b[0m ",
        config.output_style
    ))
    .to_lowercase();
    if style == "fancy" || style == "simple" {
        config.output_style = style;
    }

    // Save config
    if let Err(e) = config.save() {
        print_warning(&format!("Failed to save config: {}", e));
    } else {
        println!();
        print_success(&format!("Config saved to {}", config_file().display()));
    }

    // Shell alias
    println!();
    let install_choice =
        prompt_input("\x1b[1mInstall '??' shell alias? (Y/n):\x1b[0m ").to_lowercase();

    if install_choice.is_empty() || install_choice == "y" || install_choice == "yes" {
        let shell = detect_shell();
        let rc_file = get_rc_file(&shell);
        if install_alias(Some(&shell)) {
            print_success(&format!("Alias installed in {}", rc_file.display()));
            let reload_cmd = if shell == "powershell" {
                format!(". {}", rc_file.display())
            } else {
                format!("source {}", rc_file.display())
            };
            print_dim(&format!("Restart your shell or run: {}", reload_cmd));
        } else {
            print_dim(&format!("Alias already installed in {}.", rc_file.display()));
        }
    }

    // Summary
    println!();
    print_success("Setup complete!");
    println!();
    print_dim("Next steps:");
    if provider == Provider::Ollama {
        println!(
            "  \x1b[36mcmd-helper --create-model\x1b[0m  Create the AI model on your Ollama server"
        );
    }
    println!(
        "  \x1b[36m?? find files larger than 100MB\x1b[0m  Try your first query"
    );
}
