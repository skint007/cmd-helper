use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::config::{Config, config_file};
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

fn get_rc_file(shell: &str) -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    match shell {
        "zsh" => home.join(".zshrc"),
        "fish" => home.join(".config").join("fish").join("config.fish"),
        "powershell" => {
            if let Ok(profile) = env::var("USERPROFILE") {
                PathBuf::from(profile)
                    .join("Documents")
                    .join("PowerShell")
                    .join("Microsoft.PowerShell_profile.ps1")
            } else {
                home.join("Documents")
                    .join("PowerShell")
                    .join("Microsoft.PowerShell_profile.ps1")
            }
        }
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

pub fn interactive_setup(config: &mut Config) {
    println!();
    println!("\x1b[1;36mcmd-helper setup\x1b[0m");
    println!("\x1b[2mConfigure your command-line AI assistant.\x1b[0m");
    println!();

    // Ollama API URL (required)
    loop {
        let default_hint = if config.ollama_api_url.is_empty() {
            String::new()
        } else {
            format!(" [{}]", config.ollama_api_url)
        };
        let url = prompt_input(&format!("\x1b[1mOllama API URL{}:\x1b[0m ", default_hint));

        if url.is_empty() && !config.ollama_api_url.is_empty() {
            break;
        }
        if !url.is_empty() {
            config.ollama_api_url = url;
            break;
        }
        print_warning("URL is required.");
    }

    // Model name
    let model = prompt_input(&format!(
        "\x1b[1mModel name [{}]:\x1b[0m ",
        config.ollama_model
    ));
    if !model.is_empty() {
        config.ollama_model = model;
    }

    // API key
    if let Some(ref key) = config.ollama_api_key {
        let masked = format!("{}****", &key[..key.len().min(4)]);
        let api_key = prompt_input(&format!("\x1b[1mAPI key [{}]:\x1b[0m ", masked));
        if !api_key.is_empty() {
            config.ollama_api_key = Some(api_key);
        }
    } else {
        let api_key = prompt_input("\x1b[1mAPI key (Enter to skip):\x1b[0m ");
        if !api_key.is_empty() {
            config.ollama_api_key = Some(api_key);
        }
    }

    // Timeout
    let timeout_str = prompt_input(&format!(
        "\x1b[1mRequest timeout in seconds [{}]:\x1b[0m ",
        config.ollama_timeout
    ));
    if !timeout_str.is_empty() {
        if let Ok(t) = timeout_str.parse() {
            config.ollama_timeout = t;
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
    if let Err(e) = config.save_default_config() {
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
    println!(
        "  \x1b[36mcmd-helper --create-model\x1b[0m  Create the AI model on your Ollama server"
    );
    println!(
        "  \x1b[36m?? find files larger than 100MB\x1b[0m  Try your first query"
    );
}
