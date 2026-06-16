use std::io::{self, Write};

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::style::{Attribute, Color, Print, SetAttribute, SetForegroundColor, ResetColor};
use crossterm::{execute, terminal};
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use tokio::sync::mpsc;

#[allow(dead_code)]
pub struct ParsedResponse {
    pub command: String,
    pub explanation: String,
    pub raw: String,
}

pub fn parse_response(raw_text: &str) -> ParsedResponse {
    let text = raw_text.trim();
    if text.is_empty() {
        return ParsedResponse {
            command: String::new(),
            explanation: String::new(),
            raw: raw_text.to_string(),
        };
    }

    let re = Regex::new(r"(?m)\n\s*EXPLANATION\s*\n").unwrap();
    if let Some(mat) = re.find(text) {
        let command_section = &text[..mat.start()];
        let explanation_section = text[mat.end()..].trim().to_string();
        let cmd_re = Regex::new(r"(?m)^\s*COMMAND\s*\n").unwrap();
        let command = cmd_re.replace(command_section, "").trim().to_string();
        return ParsedResponse {
            command,
            explanation: explanation_section,
            raw: raw_text.to_string(),
        };
    }

    // Fallback
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.first().map(|s| s.trim()).unwrap_or("");
    if first.eq_ignore_ascii_case("COMMAND") && lines.len() > 1 {
        let command = lines[1].trim().to_string();
        let explanation = lines[2..].join("\n").trim().to_string();
        ParsedResponse {
            command,
            explanation,
            raw: raw_text.to_string(),
        }
    } else {
        let command = first.to_string();
        let explanation = lines[1..].join("\n").trim().to_string();
        ParsedResponse {
            command,
            explanation,
            raw: raw_text.to_string(),
        }
    }
}

fn print_colored(color: Color, text: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, SetForegroundColor(color), Print(text), ResetColor);
}

fn print_bold(text: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        SetAttribute(Attribute::Bold),
        Print(text),
        SetAttribute(Attribute::Reset)
    );
}

fn render_command_fancy(command: &str) {
    let mut stdout = io::stdout();
    let sep = "\u{2500}".repeat(9);

    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Green),
        Print(format!("{} ", sep)),
        SetAttribute(Attribute::Bold),
        Print("Command"),
        SetAttribute(Attribute::Reset),
        SetForegroundColor(Color::Green),
        Print(format!(" {}\n", sep)),
        ResetColor,
    );
    println!();

    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Cyan),
        SetAttribute(Attribute::Bold),
        Print(command),
        SetAttribute(Attribute::Reset),
        ResetColor,
    );
    println!();
}

fn render_explanation_fancy(explanation: &str) {
    let mut stdout = io::stdout();
    let sep = "\u{2500}".repeat(7);

    println!();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Blue),
        Print(format!("{} ", sep)),
        SetAttribute(Attribute::Bold),
        Print("Explanation"),
        SetAttribute(Attribute::Reset),
        SetForegroundColor(Color::Blue),
        Print(format!(" {}\n", sep)),
        ResetColor,
    );
    println!();

    let sub_bullet_re = Regex::new(r"^\s{2,}[-\u{25c6}]").unwrap();
    let top_bullet_re = Regex::new(r"^[-\u{25cb}]").unwrap();

    for line in explanation.lines() {
        let stripped = line.trim_end();
        if sub_bullet_re.is_match(stripped) {
            let content = Regex::new(r"^\s+[-\u{25c6}]\s*")
                .unwrap()
                .replace(stripped, "");
            print_colored(Color::Cyan, "  \u{25c6} ");
            println!("{}", content);
        } else if top_bullet_re.is_match(stripped) {
            let content = Regex::new(r"^[-\u{25cb}]\s*")
                .unwrap()
                .replace(stripped, "");
            print_colored(Color::Yellow, "\u{25cb} ");
            println!("{}", content);
        } else if !stripped.is_empty() {
            println!("{}", stripped);
        }
    }
}

fn render_command_simple(command: &str) {
    let sep = "\u{2500}".repeat(20);
    println!(" {} Command {}", sep, sep);
    println!();
    println!("{}", command);
}

fn render_explanation_simple(explanation: &str) {
    let sep = "\u{2500}".repeat(18);
    println!(" {} Explanation {}", sep, sep);

    let sub_bullet_re = Regex::new(r"^\s{2,}[-\u{25c6}]").unwrap();
    let top_bullet_re = Regex::new(r"^[-\u{25cb}]").unwrap();

    for line in explanation.lines() {
        let stripped = line.trim_end();
        if sub_bullet_re.is_match(stripped) {
            let content = Regex::new(r"^\s+[-\u{25c6}]\s*")
                .unwrap()
                .replace(stripped, "");
            println!("  \u{25c6} {}", content);
        } else if top_bullet_re.is_match(stripped) {
            let content = Regex::new(r"^[-\u{25cb}]\s*")
                .unwrap()
                .replace(stripped, "");
            println!("\u{25cb} {}", content);
        } else if !stripped.is_empty() {
            println!("{}", stripped);
        }
    }
}

pub async fn stream_and_display(
    mut rx: mpsc::Receiver<String>,
    output_style: &str,
    label: &str,
) -> ParsedResponse {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["\u{28fe}", "\u{28f7}", "\u{28ef}", "\u{28df}", "\u{287f}", "\u{28bf}", "\u{28fb}", "\u{28fd}"])
            .template("{spinner} {msg}")
            .unwrap(),
    );
    spinner.set_message(format!("Connecting to {}...", label));
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let mut accumulated = Vec::new();
    let mut first_token = true;
    let start = std::time::Instant::now();
    let mut showed_loading = false;

    loop {
        let tick = std::time::Duration::from_secs(1);
        match tokio::time::timeout(tick, rx.recv()).await {
            Ok(Some(token)) => {
                if first_token {
                    spinner.set_message("Generating response...");
                    first_token = false;
                }
                accumulated.push(token);
            }
            Ok(None) => break, // stream ended
            Err(_) => {
                // No token received within 1s - still waiting
                if first_token {
                    let elapsed = start.elapsed().as_secs();
                    if elapsed >= 5 && !showed_loading {
                        spinner.set_message("Still working (warming up the model may take a moment)...");
                        showed_loading = true;
                    } else if !showed_loading {
                        spinner.set_message("Waiting for response...");
                    }
                }
            }
        }
    }

    spinner.finish_and_clear();

    let raw_text = accumulated.join("");
    if raw_text.trim().is_empty() {
        print_colored(Color::Yellow, "No response received from the model.\n");
        return ParsedResponse {
            command: String::new(),
            explanation: String::new(),
            raw: String::new(),
        };
    }

    let parsed = parse_response(&raw_text);

    println!();
    if output_style == "simple" {
        render_command_simple(&parsed.command);
        render_explanation_simple(&parsed.explanation);
    } else {
        render_command_fancy(&parsed.command);
        render_explanation_fancy(&parsed.explanation);
    }

    parsed
}

pub fn display_error(error: &crate::client::ClientError) {
    use crate::client::ClientError;
    let (title, color) = match error {
        ClientError::Connection(_) => ("Connection Error", Color::Red),
        ClientError::ModelNotFound(_) => ("Model Not Found", Color::Yellow),
        ClientError::Timeout(_) => ("Timeout", Color::Yellow),
        ClientError::Auth(_) => ("Authentication Error", Color::Red),
        ClientError::Api(_) => ("API Error", Color::Red),
    };
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(color),
        SetAttribute(Attribute::Bold),
        Print(format!("[{}] ", title)),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print(format!("{}\n", error)),
    );
}

pub fn prompt_action() -> Option<&'static str> {
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        Print("\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("Press "),
        SetAttribute(Attribute::Bold),
        Print("c"),
        SetAttribute(Attribute::Reset),
        SetForegroundColor(Color::DarkGrey),
        Print(" to copy, "),
        SetAttribute(Attribute::Bold),
        Print("r"),
        SetAttribute(Attribute::Reset),
        SetForegroundColor(Color::DarkGrey),
        Print(" to run, or "),
        SetAttribute(Attribute::Bold),
        Print("Enter"),
        SetAttribute(Attribute::Reset),
        SetForegroundColor(Color::DarkGrey),
        Print(" to dismiss"),
        ResetColor,
    );
    let _ = stdout.flush();

    let _ = terminal::enable_raw_mode();
    let result = loop {
        if let Ok(Event::Key(KeyEvent { code, .. })) = event::read() {
            break match code {
                KeyCode::Char('c') | KeyCode::Char('C') => Some("copy"),
                KeyCode::Char('r') | KeyCode::Char('R') => Some("run"),
                KeyCode::Enter | KeyCode::Esc => None,
                _ => None,
            };
        }
    };
    let _ = terminal::disable_raw_mode();
    println!();
    result
}

pub fn print_success(msg: &str) {
    print_colored(Color::Green, msg);
    println!();
}

pub fn print_warning(msg: &str) {
    print_colored(Color::Yellow, msg);
    println!();
}

pub fn print_dim(msg: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print(msg),
        ResetColor,
    );
    println!();
}

pub fn print_history_table(entries: &[crate::history::HistoryEntry]) {
    if entries.is_empty() {
        print_dim("No history found.");
        return;
    }

    print_bold("Command History\n");
    println!("{:<19}  {:<30}  {}", "Time", "Query", "Command");
    println!("{}", "\u{2500}".repeat(80));
    for entry in entries {
        let ts = if entry.timestamp.len() >= 19 {
            &entry.timestamp[..19]
        } else {
            &entry.timestamp
        };
        let _ = execute!(
            io::stdout(),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("{:<19}", ts)),
            ResetColor,
            Print("  "),
            SetForegroundColor(Color::Cyan),
            Print(format!("{:<30}", entry.query)),
            ResetColor,
            Print("  "),
            SetForegroundColor(Color::Green),
            Print(&entry.command),
            ResetColor,
        );
        println!();
    }
}
