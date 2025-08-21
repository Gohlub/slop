# slop

A project manager to keep all your slop in one place.

## What it does

Slop keeps your projects organized without getting in your way. Browse, create, clone, delete, and jump between projects instantly. Opens everything in Claude Code by default. When you close your editor, jot down quick thoughts that get saved to your project folder.

## Quick Start

```bash
cargo build --release

# Add to your shell config
echo 'eval "$(./target/release/slop init ~/src/slop)"' >> ~/.zshrc  # for zsh
# echo 'eval "$(./target/release/slop init ~/src/slop)"' >> ~/.bashrc  # for bash
source ~/.zshrc  # or source ~/.bashrc for bash
```

## Usage

```bash
slop                          # Interactive project browser
slop my-new-idea              # Create or find project
slop torvalds/linux           # Clone Linux kernel repo
```

**In the navigator:**
- `↑↓` Navigate projects
- `Enter` Open project in Claude
- `D` Delete project
- `ESC` Clear search / Exit
- `⚙️ Configure` for settings

## Features

- **Smart search** - fuzzy matching with recency scoring
- **GitHub cloning** - paste any URL format (full URL, github.com/user/repo, or user/repo)
- **Project templates** - Rust, Python, JavaScript, TypeScript, Go, or blank
- **Quick notes** - capture thoughts when you close your editor  

## Configuration

```bash
slop config show                    # View current settings
slop config editor claude           # Set editor (default: claude)
slop config editor cursor           # Or use Cursor
slop config editor "code --wait"    # VS Code with flags
slop config path ~/code/projects    # Set projects directory
```

**Default settings:**
- **Projects path**: `~/src/slop`
- **Editor**: `claude`
- **Config file**: `~/.config/slop/config.toml`

## GitHub Integration

Just paste any GitHub URL format:
```bash
slop https://github.com/microsoft/vscode    # Full URL
slop github.com/facebook/react              # Without https
slop torvalds/linux                         # Shorthand
```

## Contribution
Not accepting contributions at this time.