mod client;
mod config;
mod display;
mod history;
mod setup;
mod system_info;

use std::io::IsTerminal;
use std::process;

use clap::Parser;

use client::LlmClient;
use config::{Config, Overrides, Provider};
use display::{
    display_error, print_dim, print_history_table, print_success, print_warning,
    prompt_action, stream_and_display,
};
use history::HistoryDB;
use setup::{install_alias, interactive_setup, remove_alias};
use system_info::detect_system_info;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "cmd-helper",
    about = "AI-powered command-line helper.\n\nAsk a question in natural language and get the right shell command.\n\nExamples:\n  cmd-helper find files larger than 100MB\n  cmd-helper how to compress a directory with tar\n  ?? list all docker containers",
    disable_version_flag = true,
)]
struct Cli {
    /// Natural language query
    #[arg(trailing_var_arg = true)]
    query: Vec<String>,

    /// Print version and check for updates
    #[arg(short = 'V', long = "version")]
    version: bool,

    /// Provider: ollama, openai, anthropic, deepseek, openai-compatible
    #[arg(short = 'p', long)]
    provider: Option<String>,

    /// Model name
    #[arg(short, long)]
    model: Option<String>,

    /// API base URL
    #[arg(short, long)]
    url: Option<String>,

    /// API key (overrides config for this run)
    #[arg(long = "api-key")]
    api_key: Option<String>,

    /// Request timeout (seconds)
    #[arg(short, long)]
    timeout: Option<u64>,

    /// Execute the suggested command
    #[arg(short, long)]
    run: bool,

    /// Copy command to clipboard
    #[arg(short, long)]
    copy: bool,

    /// Show recent command history
    #[arg(short = 'H', long)]
    history: bool,

    /// Search command history
    #[arg(short = 's', long = "history-search")]
    history_search: Option<String>,

    /// Clear command history
    #[arg(long = "history-clear")]
    history_clear: bool,

    /// Create/update the command-helper model on the remote Ollama server
    #[arg(long = "create-model")]
    create_model: bool,

    /// Base model to use with --create-model (default: gpt-oss:20b)
    #[arg(long = "base-model")]
    base_model: Option<String>,

    /// Create default config file
    #[arg(long = "init-config")]
    init_config: bool,

    /// Skip OS/shell detection
    #[arg(long = "no-detect")]
    no_detect: bool,

    /// Use simple output format
    #[arg(long)]
    simple: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let overrides = Overrides {
        provider: cli.provider.clone(),
        model: cli.model.clone(),
        api_url: cli.url.clone(),
        api_key: cli.api_key.clone(),
        timeout: cli.timeout,
    };

    let mut config = Config::load(overrides);

    if cli.version {
        print_version().await;
        return;
    }

    if cli.init_config {
        if let Err(e) = config.save() {
            print_warning(&format!("Failed to save config: {}", e));
            process::exit(1);
        }
        print_success(&format!(
            "Config written to {}",
            config::config_file().display()
        ));
        return;
    }

    // Subcommand routing
    if let Some(first) = cli.query.first() {
        match first.as_str() {
            "setup" => {
                interactive_setup(&mut config);
                return;
            }
            "remove-alias" => {
                if remove_alias(None) {
                    print_success("Alias removed. Restart your shell to apply.");
                } else {
                    print_warning("No cmd-helper alias found in shell config.");
                }
                return;
            }
            "install-alias" => {
                if install_alias(None) {
                    print_success("Alias installed. Restart your shell to apply.");
                } else {
                    print_warning("Alias already installed.");
                }
                return;
            }
            _ => {}
        }
    }

    if cli.create_model {
        if config.provider() != Some(Provider::Ollama) {
            print_warning("--create-model is only supported for the Ollama provider.");
            print_dim("Cloud providers (OpenAI, Anthropic, DeepSeek) use hosted models — no setup needed.");
            process::exit(1);
        }
        if !config.is_configured() {
            warn_not_configured(&config);
            process::exit(1);
        }
        create_model_cmd(&config, cli.base_model.as_deref()).await;
        return;
    }

    if cli.history_clear {
        match HistoryDB::open(&config.history_db_path) {
            Ok(db) => match db.clear() {
                Ok(count) => print_warning(&format!("Cleared {} history entries.", count)),
                Err(e) => print_warning(&format!("Failed to clear history: {}", e)),
            },
            Err(e) => print_warning(&format!("Failed to open history: {}", e)),
        }
        return;
    }

    if cli.history || cli.history_search.is_some() {
        show_history(&config, cli.history_search.as_deref());
        return;
    }

    if cli.query.is_empty() {
        print_warning("Usage: cmd-helper <your question>");
        print_dim("Example: cmd-helper find files larger than 100MB");
        process::exit(1);
    }

    if !config.is_configured() {
        warn_not_configured(&config);
        process::exit(1);
    }

    let query_text = cli.query.join(" ");

    // Detect system info
    let system_context = if cli.no_detect {
        String::new()
    } else {
        let info = detect_system_info();
        info.to_context_string()
    };

    // Query Ollama
    let style = if cli.simple {
        "simple"
    } else {
        &config.output_style
    };

    let client = match LlmClient::new(&config) {
        Ok(c) => c,
        Err(e) => {
            display_error(&e);
            process::exit(1);
        }
    };
    let rx = match client.generate_stream(&query_text, &system_context).await {
        Ok(rx) => rx,
        Err(e) => {
            display_error(&e);
            process::exit(1);
        }
    };

    let label = config
        .provider()
        .map(|p| p.display_name())
        .unwrap_or("the model");
    let parsed = stream_and_display(rx, style, label).await;

    if parsed.command.is_empty() {
        process::exit(1);
    }

    // Save to history
    if config.history_enabled {
        if let Ok(db) = HistoryDB::open(&config.history_db_path) {
            let _ = db.add(&query_text, &parsed.command, &parsed.explanation);
        }
    }

    // Post-command actions
    if cli.copy {
        copy_to_clipboard(&parsed.command);
        return;
    }

    if cli.run {
        execute_command(&parsed.command);
        return;
    }

    // Interactive prompt (only on a real terminal, if enabled)
    if config.prompt_actions && std::io::stdout().is_terminal() {
        if let Some(action) = prompt_action() {
            match action {
                "copy" => copy_to_clipboard(&parsed.command),
                "run" => execute_command(&parsed.command),
                _ => {}
            }
        }
    }
}

fn show_history(config: &Config, search_term: Option<&str>) {
    let db = match HistoryDB::open(&config.history_db_path) {
        Ok(db) => db,
        Err(e) => {
            print_warning(&format!("Failed to open history: {}", e));
            return;
        }
    };

    let entries = if let Some(term) = search_term {
        db.search(term, 20)
    } else {
        db.recent(20)
    };

    match entries {
        Ok(entries) => print_history_table(&entries),
        Err(e) => print_warning(&format!("Failed to read history: {}", e)),
    }
}

fn copy_to_clipboard(command: &str) {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(command) {
            Ok(()) => print_success("Command copied to clipboard."),
            Err(e) => {
                print_warning(&format!("Could not copy to clipboard: {}", e));
                print_dim("Install xclip or xsel on Linux.");
            }
        },
        Err(e) => {
            print_warning(&format!("Could not access clipboard: {}", e));
            print_dim("Install xclip or xsel on Linux.");
        }
    }
}

fn execute_command(command: &str) {
    println!();
    println!("\x1b[1mRunning:\x1b[0m \x1b[36m{}\x1b[0m", command);
    println!();

    let status = process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .status();

    match status {
        Ok(s) if !s.success() => {
            if let Some(code) = s.code() {
                print_warning(&format!("Command exited with code {}", code));
            }
        }
        Err(e) => print_warning(&format!("Failed to execute command: {}", e)),
        _ => {}
    }
}

fn warn_not_configured(config: &Config) {
    match config.provider() {
        None => {
            print_warning(&format!("Unknown provider '{}'.", config.provider));
            print_dim("Valid providers: ollama, openai, anthropic, deepseek, openai-compatible");
        }
        Some(p) if p.requires_api_key() => {
            print_warning(&format!("No API key configured for {}.", p.display_name()));
            print_dim("Run: cmd-helper setup");
        }
        Some(p) => {
            print_warning(&format!("{} is not configured.", p.display_name()));
            print_dim("Run: cmd-helper setup");
        }
    }
}

async fn create_model_cmd(config: &Config, base_model: Option<&str>) {
    let model_name = config.active_model();
    let base = base_model.unwrap_or("gpt-oss:20b");
    println!(
        "Creating model \x1b[36m{}\x1b[0m from \x1b[36m{}\x1b[0m on \x1b[36m{}\x1b[0m...",
        model_name,
        base,
        config.active_api_url()
    );

    let client = match LlmClient::new(config) {
        Ok(c) => c,
        Err(e) => {
            display_error(&e);
            process::exit(1);
        }
    };
    let mut rx = match client.create_model(&model_name, base).await {
        Ok(rx) => rx,
        Err(e) => {
            display_error(&e);
            process::exit(1);
        }
    };

    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner} {msg}")
            .unwrap(),
    );
    spinner.set_message("Creating model (this may take a while if the base model needs to be pulled)...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    while let Some(status_msg) = rx.recv().await {
        spinner.set_message(status_msg);
    }

    spinner.finish_and_clear();
    println!();
    print_success(&format!("Model '{}' created successfully!", model_name));
}

async fn print_version() {
    println!("cmd-helper v{}", VERSION);

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .ok();

    let latest = async {
        let resp = client?
            .get("https://api.github.com/repos/skint007/cmd-helper/releases/latest")
            .header("User-Agent", "cmd-helper")
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .ok()?;
        let json: serde_json::Value = resp.json().await.ok()?;
        json["tag_name"].as_str().map(|s| s.trim_start_matches('v').to_string())
    }
    .await;

    match latest {
        Some(ref tag) if tag != VERSION => {
            println!("Latest:  v{}", tag);
        }
        Some(_) => {
            print_success("You are on the latest version.");
        }
        None => {
            print_dim("Could not check for updates.");
        }
    }
}
