# cmd-helper

AI-powered command-line helper that translates natural language into shell commands using [Ollama](https://ollama.com).

Ask a question in plain English and get the right command with an explanation.

```
$ ?? find files larger than 100MB

───────── Command ─────────

  find . -size +100M

─────── Explanation ───────

○ find searches for files in a directory hierarchy.
  ◆ . starts from the current directory.
  ◆ -size +100M matches files larger than 100 megabytes.

[C]opy  [R]un  [Enter] skip
```

## Features

- Natural language to shell command translation
- Streaming response display with formatted output
- OS and shell auto-detection for context-aware suggestions
- Interactive prompt to copy or run the suggested command
- Command history with search (stored in SQLite)
- Platform-specific: Arch Linux/bash on Linux, PowerShell on Windows
- `??` shell alias for quick access

## Requirements

- An [Ollama](https://ollama.com) server (local or remote)

## Installation

### From source

```sh
cargo install --path .
```

### Arch Linux (AUR)

```sh
cd aur
makepkg -si
```

### GitHub Releases

Pre-built binaries for Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64) are available on the [Releases](https://github.com/skint007/cmd-helper/releases) page.

## Setup

Run the interactive setup to configure your Ollama connection and install the `??` alias:

```sh
cmd-helper setup
```

This will prompt for:
- Ollama API URL (e.g. `http://localhost:11434`)
- Model name (default: `command-helper` on Linux, `command-helper-windows` on Windows)
- API key (optional, for remote/authenticated servers)
- Request timeout
- Output style (fancy or simple)
- `??` shell alias installation (bash, zsh, fish, PowerShell)

Then create the AI model on your Ollama server:

```sh
cmd-helper --create-model
```

## Usage

```sh
# Using the alias
?? how to compress a directory with tar
?? list all docker containers
?? find and replace text in all .py files

# Using the full command
cmd-helper show disk usage sorted by size

# Copy or run directly
cmd-helper -c show my public IP     # copy to clipboard
cmd-helper -r list open ports       # run immediately
```

### CLI Options

```
Usage: cmd-helper [OPTIONS] [QUERY]...

Options:
  -m, --model <MODEL>              Ollama model name
  -u, --url <URL>                  Ollama API URL
  -t, --timeout <TIMEOUT>          Request timeout (seconds)
  -r, --run                        Execute the suggested command
  -c, --copy                       Copy command to clipboard
  -H, --history                    Show recent command history
  -s, --history-search <TERM>      Search command history
      --history-clear              Clear command history
      --create-model               Create the AI model on the Ollama server
      --init-config                Create default config file
      --no-detect                  Skip OS/shell detection
      --simple                     Use simple output format
  -h, --help                       Print help
  -V, --version                    Print version
```

### Subcommands

```sh
cmd-helper setup            # Interactive setup wizard
cmd-helper install-alias    # Install the ?? alias
cmd-helper remove-alias     # Remove the ?? alias
```

## Configuration

Config is stored at `~/.config/cmd-helper/config.yaml`:

```yaml
ollama_model: command-helper
ollama_api_url: http://localhost:11434
ollama_api_key: null
ollama_timeout: 10
output_style: fancy
history_enabled: true
```

Environment variables override the config file:

| Variable | Description |
|---|---|
| `CMD_HELPER_MODEL` | Model name |
| `CMD_HELPER_API_URL` | Ollama API URL |
| `CMD_HELPER_API_KEY` | API key |
| `CMD_HELPER_TIMEOUT` | Timeout in seconds |

## License

[MIT](LICENSE)
