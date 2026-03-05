mod client;
mod config;
mod display;
mod history;
mod setup;
mod system_info;

use std::collections::HashMap;
use std::io::IsTerminal;
use std::process;

use clap::Parser;

use client::OllamaClient;
use config::Config;
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
    version = VERSION,
)]
struct Cli {
    /// Natural language query
    #[arg(trailing_var_arg = true)]
    query: Vec<String>,

    /// Ollama model name
    #[arg(short, long)]
    model: Option<String>,

    /// Ollama API URL
    #[arg(short, long)]
    url: Option<String>,

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

    /// Create the command-helper model on the remote Ollama server
    #[arg(long = "create-model")]
    create_model: bool,

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

    let mut overrides = HashMap::new();
    overrides.insert(
        "ollama_model",
        cli.model.clone(),
    );
    overrides.insert(
        "ollama_api_url",
        cli.url.clone(),
    );
    overrides.insert(
        "ollama_timeout",
        cli.timeout.map(|t| t.to_string()),
    );

    let mut config = Config::load(overrides);

    if cli.init_config {
        if let Err(e) = config.save_default_config() {
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
        if !config.is_configured() {
            print_warning("No Ollama URL configured.");
            print_dim("Run: cmd-helper setup");
            process::exit(1);
        }
        create_model_cmd(&config).await;
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
        print_warning("No Ollama URL configured.");
        print_dim("Run: cmd-helper setup");
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

    let ollama = OllamaClient::new(&config);
    let rx = match ollama.generate_stream(&query_text, &system_context).await {
        Ok(rx) => rx,
        Err(e) => {
            display_error(&e);
            process::exit(1);
        }
    };

    let parsed = stream_and_display(rx, style).await;

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

    // Interactive prompt (only on a real terminal)
    if std::io::stdout().is_terminal() {
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

async fn create_model_cmd(config: &Config) {
    let model_name = &config.ollama_model;
    println!(
        "Creating model \x1b[36m{}\x1b[0m on \x1b[36m{}\x1b[0m...",
        model_name, config.ollama_api_url
    );

    let ollama = OllamaClient::new(config);
    let mut rx = match ollama.create_model(model_name).await {
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
