# try - fresh directories for every vibe (but written in *Rust*)

> made it (oneshot with AI) because I wanted to modify this script for my own usecases, I don't know Ruby but I know Rust so it will be easier for me to work on it

# What it does 

[![asciicast](https://asciinema.org/a/ve8AXBaPhkKz40YbqPTlVjqgs.svg)](https://asciinema.org/a/ve8AXBaPhkKz40YbqPTlVjqgs)

Instantly navigate through all your experiment directories with:
- **Fuzzy search** that just works
- **Smart sorting** - recently used stuff bubbles to the top
- **Auto-dating** - creates directories like `2025-08-17-redis-experiment` (this will definitively change)
- **Easy config** - just one *Rust* file, at least 6 dependancies (will probably grow, I won't spend time writing it purely in Rust)

## Quick Start

```bash
# Clone and build
git clone https://github.com/Gohlub/try-rs
cd try-rs
cargo build --release

# Add to your shell (bash/zsh)
echo 'eval "$(./target/release/try init ~/src/tries)"' >> ~/.zshrc
# Or install globally
cargo install --path .
echo 'eval "$(try init ~/src/tries)"' >> ~/.zshrc
```
## Outline

All your experiments in one place, with instant fuzzy search:

```bash
$ try pool
→ 2025-08-14-redis-connection-pool    2h, 18.5
  2025-08-03-thread-pool              3d, 12.1
  2025-07-22-db-pooling               2w, 8.3
  + Create new: pool
```

Type, arrow down, enter. You're there.

## Features

### 🎯 Smart Fuzzy Search
Not just substring matching - it's smart:
- `rds` matches `redis-server`
- `connpool` matches `connection-pool`
- Recent stuff scores higher
- Shorter names win on equal matches

### ⏰ Time-Aware
- Shows how long ago you touched each project
- Recently accessed directories float to the top
- Perfect for "what was I working on yesterday?"

### 🎨 Pretty TUI
- Clean, minimal interface
- Highlights matches as you type
- Shows scores so you know why things are ranked
- Dark mode by default (because obviously)

### 📁 Organized Chaos
- Everything lives in `~/src/tries` (configurable via `TRY_PATH`)
- Auto-prefixes with dates: `2025-08-17-your-idea`
- Skip the date prompt if you already typed a name

### Shell Integration

Add to your `~/.bashrc` or `~/.zshrc`:


```bash
# default is ~/src/tries
eval "$(try init)"
```

Or if you want to customize the location:

```bash
eval "$(try init ~/src/tries)"
```

## Usage

```bash
try                 # Browse all experiments
try redis           # Jump to redis experiment or create new
try new api         # Start with "2025-08-17-new-api"
try --help          # See all options
```

### Keyboard Shortcuts

- `↑/↓` or `Ctrl-P/N` - Navigate
- `Enter` - Select or create
- `Backspace` - Delete character
- `ESC` - Cancel
- Just type to filter

## Configuration

Set `TRY_PATH` to change where experiments are stored:

```bash
export TRY_PATH=~/code/sketches
```

Default: `~/src/tries`

## Why Rust?

- That's what I need it to be

## Contributing

This will be purpose built to support vibecoding that fits my needs. I suggest forking
the original and updating/rewriting it according to your needs. 

## License

MIT - Do whatever you want with it.

