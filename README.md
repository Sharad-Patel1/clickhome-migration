# ch-migrate

A terminal-based tool for tracking TypeScript model migrations from `shared/` to `shared_2023/` in the ClickHome enterprise codebase.

## Features

- **Scan**: Analyze your codebase and get a summary of migration status
- **Watch**: Interactive TUI with real-time file watching
- **Report**: Generate JSON or CSV reports for tracking progress

## Requirements

- **Rust 1.85+** (Rust 2024 edition)
- A terminal that supports ANSI colors (most modern terminals)

## Quick Start

```bash
# Clone the repository
git clone https://github.com/Sharad-Patel1/ch-migration.git
cd ch-migration

# Build and run
cargo run -p ch-cli -- scan --path /path/to/WebApp.Desktop/src
```

## Building from Source

### Debug Build (faster compilation)

```bash
cargo build -p ch-cli
```

The binary will be at `target/debug/ch-migrate`.

### Release Build (optimized)

```bash
cargo build -p ch-cli --release
```

The binary will be at `target/release/ch-migrate`.

## Installation

### Option 1: Using `cargo install` (Recommended)

Install directly from the local source:

```bash
cargo install --path crates/ch-cli
```

This installs `ch-migrate` to `~/.cargo/bin/`, which should already be in your PATH if you have Rust installed.

### Option 2: Manual Installation

Build a release binary and copy it to a location in your PATH.

#### macOS

```bash
# Build release binary
cargo build -p ch-cli --release

# Option A: Install to /usr/local/bin (requires sudo)
sudo cp target/release/ch-migrate /usr/local/bin/

# Option B: Install to user bin directory (no sudo required)
mkdir -p ~/bin
cp target/release/ch-migrate ~/bin/

# If using Option B, add to PATH in ~/.zshrc or ~/.bash_profile:
# export PATH="$HOME/bin:$PATH"
```

#### Linux

```bash
# Build release binary
cargo build -p ch-cli --release

# Option A: Install to /usr/local/bin (requires sudo)
sudo cp target/release/ch-migrate /usr/local/bin/

# Option B: Install to ~/.local/bin (XDG standard, no sudo)
mkdir -p ~/.local/bin
cp target/release/ch-migrate ~/.local/bin/

# If using Option B, ensure ~/.local/bin is in your PATH
# Add to ~/.bashrc or ~/.zshrc:
# export PATH="$HOME/.local/bin:$PATH"
```

#### Windows

```powershell
# Build release binary
cargo build -p ch-cli --release

# Option A: Install to a directory in your PATH
# Create a bin directory if it doesn't exist
mkdir -Force "$env:USERPROFILE\bin"

# Copy the executable
copy target\release\ch-migrate.exe "$env:USERPROFILE\bin\"

# Add to PATH (run in PowerShell as Administrator, or add manually via System Properties)
[Environment]::SetEnvironmentVariable(
    "Path",
    "$env:USERPROFILE\bin;" + [Environment]::GetEnvironmentVariable("Path", "User"),
    "User"
)

# Option B: Install to Program Files (requires Administrator)
copy target\release\ch-migrate.exe "C:\Program Files\ch-migrate\"
# Then add "C:\Program Files\ch-migrate" to your system PATH
```

### Verify Installation

After installation, verify it works:

```bash
ch-migrate --version
ch-migrate --help
```

## Usage

### Environment Variable

You can set a default path using the `CH_MIGRATE_PATH` environment variable:

```bash
# Add to your shell profile (.bashrc, .zshrc, etc.)
export CH_MIGRATE_PATH="/path/to/WebApp.Desktop/src"

# Then run without --path
ch-migrate scan
```

### Commands

#### `scan` - One-shot Analysis

Scan the codebase and display a migration status summary.

```bash
# Basic scan
ch-migrate scan --path /path/to/WebApp.Desktop/src

# With detailed file list
ch-migrate scan --path /path/to/WebApp.Desktop/src --detailed

# Using environment variable
export CH_MIGRATE_PATH="/path/to/WebApp.Desktop/src"
ch-migrate scan
ch-migrate scan -d  # short flag for --detailed
```

**Example Output:**

```
Migration Status Summary
========================

Total files scanned: 1247
  Legacy:           342 (need migration)
  Partial:          28 (in progress)
  Migrated:         756 (complete)
  No models:        121 (no action needed)
  Errors:           0

Migration progress: 67.1%
Files needing work: 370
```

#### `watch` - Interactive TUI

Start the interactive terminal UI with live file watching.

```bash
# Start TUI with file watching
ch-migrate watch --path /path/to/WebApp.Desktop/src

# Start TUI without file watching (static view)
ch-migrate watch --path /path/to/WebApp.Desktop/src --no-watch
```

**TUI Keybindings:**

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit |
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `Enter` | View file details |
| `/` | Filter files |
| `?` | Show help |
| `r` | Refresh scan |
| `Tab` | Switch focus |

#### `report` - Generate Reports

Generate a migration report in JSON or CSV format.

```bash
# JSON report to stdout
ch-migrate report --path /path/to/WebApp.Desktop/src

# JSON report to file
ch-migrate report --path /path/to/WebApp.Desktop/src --output report.json

# CSV report
ch-migrate report --path /path/to/WebApp.Desktop/src --format csv --output report.csv

# Short flags
ch-migrate report -p /path/to/WebApp.Desktop/src -f csv -o report.csv
```

### Global Options

These options work with all commands:

| Option | Short | Description |
|--------|-------|-------------|
| `--path <PATH>` | `-p` | Path to WebApp.Desktop/src directory |
| `--verbose` | `-v` | Enable debug-level logging |
| `--no-color` | | Disable colored output |
| `--help` | `-h` | Show help information |
| `--version` | `-V` | Show version |

### Disabling Colors

Colors can be disabled in multiple ways:

```bash
# Using the flag
ch-migrate scan --path /path --no-color

# Using environment variable (standard NO_COLOR spec)
NO_COLOR=1 ch-migrate scan --path /path

# Permanently in shell profile
export NO_COLOR=1
```

## Uninstalling

### If installed with `cargo install`

```bash
cargo uninstall ch-cli
```

### If installed manually

Simply delete the binary:

```bash
# macOS/Linux
rm /usr/local/bin/ch-migrate
# or
rm ~/bin/ch-migrate
# or
rm ~/.local/bin/ch-migrate

# Windows (PowerShell)
Remove-Item "$env:USERPROFILE\bin\ch-migrate.exe"
```

## Troubleshooting

### "command not found: ch-migrate"

The binary is not in your PATH. Either:
1. Use the full path to the binary
2. Add the installation directory to your PATH
3. Reinstall using `cargo install --path crates/ch-cli`

### "Path does not exist" error

Ensure the path points to a valid directory:

```bash
# Check the path exists
ls /path/to/WebApp.Desktop/src

# Use absolute path
ch-migrate scan --path /Users/you/projects/WebApp.Desktop/src
```

### TUI doesn't display correctly

Ensure your terminal supports:
- ANSI escape codes
- UTF-8 encoding
- At least 80x24 terminal size

Try a different terminal emulator if issues persist:
- macOS: Terminal.app, iTerm2, Alacritty
- Linux: GNOME Terminal, Konsole, Alacritty
- Windows: Windows Terminal, PowerShell 7+

### High CPU usage in watch mode

The file watcher debounces events, but if you're seeing high CPU:

```bash
# Run without file watching
ch-migrate watch --path /path --no-watch
```

## Development

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for technical details.

```bash
# Run checks
cargo check --workspace
cargo clippy --workspace

# Run tests
cargo test --workspace

# Format code
cargo fmt --all
```

## License

MIT
