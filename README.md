# cmd-helper

AI-powered command-line helper that translates natural language into shell commands.

Works with a local/remote [Ollama](https://ollama.com) server or a cloud provider — **OpenAI**, **Anthropic**, **DeepSeek**, or any **OpenAI-compatible** endpoint (Groq, OpenRouter, LM Studio, vLLM, ...).

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
- Multiple providers: Ollama, OpenAI, Anthropic, DeepSeek, or any OpenAI-compatible endpoint
- Streaming response display with formatted output
- OS and shell auto-detection for context-aware suggestions
- Interactive prompt to copy or run the suggested command
- Command history with search (stored in SQLite)
- Platform-specific: Arch Linux/bash on Linux, PowerShell on Windows
- `??` shell alias for quick access

## Requirements

One of:

- An [Ollama](https://ollama.com) server (local or remote), **or**
- An API key for OpenAI, Anthropic, DeepSeek, or another OpenAI-compatible service

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

Run the interactive setup to choose a provider, configure credentials, and install the `??` alias:

```sh
cmd-helper setup
```

This will prompt for:
- Provider (Ollama, OpenAI, Anthropic, DeepSeek, or OpenAI-compatible)
- API key (required for cloud providers; optional for Ollama)
- Model name (sensible default per provider — e.g. `claude-opus-4-8` for Anthropic, `gpt-4o-mini` for OpenAI)
- API base URL (defaults to the provider's public endpoint; required for Ollama and OpenAI-compatible)
- Request timeout
- Output style (fancy or simple)
- `??` shell alias installation (bash, zsh, fish, PowerShell)

### Ollama

Ollama uses a small fine-tuned model. After setup, create it on your server:

```sh
cmd-helper --create-model
```

Cloud providers (OpenAI, Anthropic, DeepSeek) use hosted models, so no model creation is needed — just set an API key.

### Switching providers

Each provider's settings are stored separately, so you can switch without re-entering keys:

```sh
cmd-helper -p anthropic find files larger than 100MB   # one-off override
cmd-helper setup                                        # change the default provider
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
  -p, --provider <PROVIDER>        Provider: ollama, openai, anthropic, deepseek, openai-compatible
  -m, --model <MODEL>              Model name
  -u, --url <URL>                  API base URL
      --api-key <API_KEY>          API key (overrides config for this run)
  -t, --timeout <TIMEOUT>          Request timeout (seconds)
  -r, --run                        Execute the suggested command
  -c, --copy                       Copy command to clipboard
  -H, --history                    Show recent command history
  -s, --history-search <TERM>      Search command history
      --history-clear              Clear command history
      --create-model               Create the AI model on the Ollama server (Ollama only)
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

Config is stored at `~/.config/cmd-helper/config.yaml`. The `provider:` key selects the active
provider, and each provider's settings live under `providers:`, so you can keep several configured
at once and switch between them:

```yaml
provider: anthropic
providers:
  ollama:
    model: command-helper
    api_url: http://localhost:11434
  openai:
    model: gpt-4o-mini
    api_key: sk-...
  anthropic:
    model: claude-opus-4-8
    api_key: sk-ant-...
  deepseek:
    model: deepseek-chat
    api_key: sk-...
  openai-compatible:
    api_url: https://openrouter.ai/api/v1
    model: anthropic/claude-3.5-sonnet
    api_key: sk-or-...
timeout: 10
output_style: fancy
history_enabled: true
prompt_actions: true
```

Per provider, any unset field falls back to a default: the public API base URL, a sensible default
model, and (for cloud providers) the standard API-key environment variable. Legacy flat `ollama_*`
keys from older versions are still read and migrated automatically.

Environment variables override the config file. The generic ones apply to the **active** provider:

| Variable | Description |
|---|---|
| `CMD_HELPER_PROVIDER` | Active provider |
| `CMD_HELPER_MODEL` | Model name |
| `CMD_HELPER_API_URL` | API base URL |
| `CMD_HELPER_API_KEY` | API key |
| `CMD_HELPER_TIMEOUT` | Timeout in seconds |

Cloud providers also pick up their standard key variables when no key is set in config:
`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `DEEPSEEK_API_KEY`.

## License

[MIT](LICENSE)
