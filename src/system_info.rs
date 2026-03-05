use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
pub struct SystemInfo {
    pub os_name: String,
    pub os_version: String,
    pub distro: Option<String>,
    pub shell: String,
    pub shell_version: Option<String>,
    pub terminal: Option<String>,
}

impl SystemInfo {
    pub fn to_context_string(&self) -> String {
        let mut parts = vec![format!("OS: {}", self.os_name)];
        if let Some(ref distro) = self.distro {
            parts[0] = format!("{} ({})", parts[0], distro);
        }
        let mut shell_str = self.shell.clone();
        if let Some(ref ver) = self.shell_version {
            shell_str = format!("{} {}", shell_str, ver);
        }
        parts.push(format!("Shell: {}", shell_str));
        format!("[{}]", parts.join(" | "))
    }
}

pub fn detect_system_info() -> SystemInfo {
    let shell = detect_shell();
    let shell_version = detect_shell_version(&shell);
    SystemInfo {
        os_name: std::env::consts::OS.to_string(),
        os_version: get_os_version(),
        distro: detect_distro(),
        shell,
        shell_version,
        terminal: env::var("TERM_PROGRAM")
            .or_else(|_| env::var("TERMINAL"))
            .ok(),
    }
}

fn detect_shell() -> String {
    if let Ok(shell_path) = env::var("SHELL") {
        if let Some(name) = Path::new(&shell_path).file_name() {
            return name.to_string_lossy().to_string();
        }
    }
    if let Ok(comspec) = env::var("COMSPEC") {
        if comspec.to_lowercase().contains("powershell") {
            return "powershell".to_string();
        }
        return "cmd".to_string();
    }
    "unknown".to_string()
}

fn detect_distro() -> Option<String> {
    match std::env::consts::OS {
        "macos" => Some("macOS".to_string()),
        "linux" => {
            let content = fs::read_to_string("/etc/os-release").ok()?;
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                    return Some(rest.trim().trim_matches('"').to_string());
                }
            }
            None
        }
        _ => None,
    }
}

fn detect_shell_version(shell: &str) -> Option<String> {
    let flag = match shell {
        "bash" | "zsh" | "fish" => "--version",
        _ => return None,
    };
    let output = Command::new(shell)
        .arg(flag)
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;
    first_line
        .replace(',', "")
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))
        .map(|s| s.to_string())
}

fn get_os_version() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = Command::new("uname").arg("-r").output() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    String::new()
}
