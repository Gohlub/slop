use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{
        disable_raw_mode, enable_raw_mode, size, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use dirs::home_dir;
use is_terminal::IsTerminal;
use std::{
    env,
    fs::{self, Metadata},
    io::{self, Write},
    path::PathBuf,
    time::UNIX_EPOCH,
};

#[derive(Parser)]
#[command(name = "try-rs")]
#[command(about = "Vibecoding experiments to build at hyperspeed")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize shell function for aliasing
    Init {
        /// Path to tries directory
        #[arg(long)]
        path: Option<PathBuf>,
        /// Additional path argument (for backward compatibility)
        tries_path: Option<PathBuf>,
    },
    /// Interactive selector; prints shell cd commands
    Cd {
        /// Path to tries directory
        #[arg(long)]
        path: Option<PathBuf>,
        /// Search query
        query: Vec<String>,
    },
}

#[derive(Debug, Clone)]
struct TryDirectory {
    basename: String,
    path: PathBuf,
    ctime: DateTime<Utc>,
    mtime: DateTime<Utc>,
    score: f64,
}

struct TrySelector {
    cursor_pos: usize,
    scroll_offset: usize,
    input_buffer: String,
    selected: Option<SelectionResult>,
    term_width: u16,
    term_height: u16,
    all_tries: Option<Vec<TryDirectory>>,
    base_path: PathBuf,
}

#[derive(Debug, Clone)]
struct SelectionResult {
    selection_type: SelectionType,
    path: PathBuf,
}

#[derive(Debug, Clone)]
enum SelectionType {
    Cd,
    Mkdir,
    Cancel,
}

impl TrySelector {
    fn new(search_term: String, base_path: PathBuf) -> Result<Self> {
        let input_buffer = search_term.replace(' ', "-");
        
        // Create base directory if it doesn't exist
        fs::create_dir_all(&base_path)
            .with_context(|| format!("Failed to create base directory: {}", base_path.display()))?;

        let (term_width, term_height) = size().unwrap_or((80, 24));

        Ok(TrySelector {
            cursor_pos: 0,
            scroll_offset: 0,
            input_buffer,
            selected: None,
            term_width,
            term_height,
            all_tries: None,
            base_path,
        })
    }

    fn run(&mut self) -> Result<Option<SelectionResult>> {
        // Check if we have a TTY
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            eprintln!("Error: try requires an interactive terminal");
            return Ok(None);
        }

        self.setup_terminal()?;
        
        let result = self.main_loop();
        
        self.restore_terminal()?;
        
        result
    }

    fn setup_terminal(&mut self) -> Result<()> {
        enable_raw_mode()?;
        execute!(io::stderr(), EnterAlternateScreen, Hide, Clear(ClearType::All))?;
        self.update_terminal_size()?;
        Ok(())
    }

    fn update_terminal_size(&mut self) -> Result<()> {
        let (width, height) = size().unwrap_or((80, 24));
        self.term_width = width;
        self.term_height = height;
        Ok(())
    }

    fn restore_terminal(&self) -> Result<()> {
        execute!(
            io::stderr(),
            Show,
            LeaveAlternateScreen,
            Clear(ClearType::All)
        )?;
        disable_raw_mode()?;
        Ok(())
    }

    fn load_all_tries(&mut self) -> Result<()> {
        if self.all_tries.is_some() {
            return Ok(());
        }

        let mut tries = Vec::new();
        let entries = fs::read_dir(&self.base_path)
            .with_context(|| format!("Failed to read directory: {}", self.base_path.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                if let Some(basename) = path.file_name().and_then(|n| n.to_str()) {
                    let metadata = entry.metadata()?;
                    let (ctime, mtime) = self.get_times(&metadata)?;
                    
                    tries.push(TryDirectory {
                        basename: basename.to_string(),
                        path: path.clone(),
                        ctime,
                        mtime,
                        score: 0.0,
                    });
                }
            }
        }

        self.all_tries = Some(tries);
        Ok(())
    }

    fn get_times(&self, metadata: &Metadata) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
        let ctime = metadata
            .created()
            .or_else(|_| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        let mtime = metadata.modified().unwrap_or(UNIX_EPOCH);
        
        let ctime = DateTime::from(ctime);
        let mtime = DateTime::from(mtime);
        
        Ok((ctime, mtime))
    }

    fn get_tries(&mut self) -> Result<Vec<TryDirectory>> {
        self.load_all_tries()?;
        
        let mut scored_tries: Vec<TryDirectory> = self
            .all_tries
            .as_ref()
            .unwrap()
            .iter()
            .map(|try_dir| {
                let score = self.calculate_score(
                    &try_dir.basename,
                    &self.input_buffer,
                    &try_dir.ctime,
                    &try_dir.mtime,
                );
                let mut try_dir = try_dir.clone();
                try_dir.score = score;
                try_dir
            })
            .collect();

        // Filter and sort
        if self.input_buffer.is_empty() {
            scored_tries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        } else {
            scored_tries.retain(|t| t.score > 0.0);
            scored_tries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        }

        Ok(scored_tries)
    }

    fn calculate_score(&self, text: &str, query: &str, ctime: &DateTime<Utc>, mtime: &DateTime<Utc>) -> f64 {
        let mut score = 0.0;

        // Date prefix bonus
        if text.starts_with(|c: char| c.is_ascii_digit()) 
            && text.len() >= 10 
            && text.chars().nth(4) == Some('-') 
            && text.chars().nth(7) == Some('-') {
            score += 2.0;
        }

        // Search query matching
        if !query.is_empty() {
            let text_lower = text.to_lowercase();
            let query_lower = query.to_lowercase();
            let query_chars: Vec<char> = query_lower.chars().collect();
            
            let mut last_pos = -1i32;
            let mut query_idx = 0;

            for (pos, ch) in text_lower.chars().enumerate() {
                if query_idx >= query_chars.len() {
                    break;
                }
                if ch != query_chars[query_idx] {
                    continue;
                }

                // Base point + word boundary bonus
                score += 1.0;
                if pos == 0 || !text_lower.chars().nth(pos.saturating_sub(1)).unwrap_or('a').is_alphanumeric() {
                    score += 1.0;
                }

                // Proximity bonus
                if last_pos >= 0 {
                    let gap = pos as i32 - last_pos - 1;
                    score += 1.0 / (gap as f64 + 1.0).sqrt();
                }

                last_pos = pos as i32;
                query_idx += 1;
            }

            // Return 0 if not all query chars matched
            if query_idx < query_chars.len() {
                return 0.0;
            }

            // Density bonus
            if last_pos >= 0 {
                score *= query_chars.len() as f64 / (last_pos as f64 + 1.0);
            }

            // Length penalty
            score *= 10.0 / (text.len() as f64 + 10.0);
        }

        // Time-based scoring
        let now = Utc::now();
        
        // Creation time bonus
        let days_old = (now - *ctime).num_seconds() as f64 / 86400.0;
        score += 2.0 / (days_old + 1.0).sqrt();

        // Access time bonus
        let hours_since_access = (now - *mtime).num_seconds() as f64 / 3600.0;
        score += 3.0 / (hours_since_access + 1.0).sqrt();

        score
    }

    fn main_loop(&mut self) -> Result<Option<SelectionResult>> {
        loop {
            let tries = self.get_tries()?;
            let total_items = tries.len() + 1; // +1 for "Create new" option

            // Ensure cursor is within bounds
            self.cursor_pos = self.cursor_pos.min(total_items.saturating_sub(1));

            self.render(&tries)?;

            if let Event::Key(key) = event::read()? {
                match key {
                    KeyEvent { code: KeyCode::Up, .. } | KeyEvent { code: KeyCode::Char('p'), modifiers: KeyModifiers::CONTROL, .. } => {
                        if self.cursor_pos > 0 {
                            self.cursor_pos -= 1;
                        }
                    }
                    KeyEvent { code: KeyCode::Down, .. } | KeyEvent { code: KeyCode::Char('n'), modifiers: KeyModifiers::CONTROL, .. } => {
                        if self.cursor_pos < total_items.saturating_sub(1) {
                            self.cursor_pos += 1;
                        }
                    }
                    KeyEvent { code: KeyCode::Left, .. } | KeyEvent { code: KeyCode::Right, .. } => {
                        // Ignore arrow keys
                    }
                    KeyEvent { code: KeyCode::Enter, .. } => {
                        if self.cursor_pos < tries.len() {
                            self.handle_selection(&tries[self.cursor_pos]);
                        } else {
                            self.handle_create_new()?;
                        }
                        if self.selected.is_some() {
                            break;
                        }
                    }
                    KeyEvent { code: KeyCode::Backspace, .. } => {
                        if !self.input_buffer.is_empty() {
                            self.input_buffer.pop();
                            self.cursor_pos = 0;
                        }
                    }
                    KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. } | 
                    KeyEvent { code: KeyCode::Esc, .. } => {
                        self.selected = None;
                        break;
                    }
                    KeyEvent { code: KeyCode::Char(ch), .. } => {
                        if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ' ' {
                            self.input_buffer.push(ch);
                            self.cursor_pos = 0;
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(self.selected.clone())
    }

    fn render(&mut self, tries: &[TryDirectory]) -> Result<()> {
        execute!(io::stderr(), Clear(ClearType::All), MoveTo(0, 0))?;

        let separator = "─".repeat(self.term_width as usize - 1);

        // Header
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Yellow),
            Print("📁 Try Directory Selection"),
            ResetColor,
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Search input
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Yellow),
            Print("Search: "),
            ResetColor,
            Print(&self.input_buffer),
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Calculate visible window
        let max_visible = (self.term_height as usize).saturating_sub(8).max(3);
        let total_items = tries.len() + 1;

        // Adjust scroll window
        if self.cursor_pos < self.scroll_offset {
            self.scroll_offset = self.cursor_pos;
        } else if self.cursor_pos >= self.scroll_offset + max_visible {
            self.scroll_offset = self.cursor_pos.saturating_sub(max_visible - 1);
        }

        // Display items
        let visible_end = (self.scroll_offset + max_visible).min(total_items);

        for idx in self.scroll_offset..visible_end {
            // Add blank line before "Create new"
            if idx == tries.len() && !tries.is_empty() && idx >= self.scroll_offset {
                execute!(io::stderr(), Print("\r\n"))?;
            }

            // Print cursor/selection indicator
            let is_selected = idx == self.cursor_pos;
            if is_selected {
                execute!(io::stderr(), SetForegroundColor(Color::Yellow), Print("→ "), ResetColor)?;
            } else {
                execute!(io::stderr(), Print("  "))?;
            }

            // Display try directory or "Create new" option
            if idx < tries.len() {
                let try_dir = &tries[idx];
                self.render_try_directory(try_dir, is_selected)?;
            } else {
                self.render_create_new_option(is_selected)?;
            }

            execute!(io::stderr(), Print("\r\n"))?;
        }

        // Scroll indicator if needed
        if total_items > max_visible {
            execute!(
                io::stderr(),
                SetForegroundColor(Color::DarkGrey),
                Print(&separator),
                Print("\r\n"),
                Print(&format!("[{}-{}/{}]", self.scroll_offset + 1, visible_end, total_items)),
                ResetColor,
                Print("\r\n"),
            )?;
        }

        // Instructions at bottom
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            Print("\r\n"),
            Print("↑↓: Navigate  Enter: Select  ESC: Cancel"),
            ResetColor,
        )?;

        io::stderr().flush()?;
        Ok(())
    }

    fn render_try_directory(&self, try_dir: &TryDirectory, is_selected: bool) -> Result<()> {
        execute!(io::stderr(), Print("📁 "))?;

        if is_selected {
            execute!(io::stderr(), SetForegroundColor(Color::Black))?;
        }

        // Format directory name with date styling
        if let Some(captures) = self.parse_date_prefix(&try_dir.basename) {
            let (date_part, name_part) = captures;
            
            // Render the date part (faint)
            execute!(io::stderr(), SetForegroundColor(Color::DarkGrey), Print(&date_part), ResetColor)?;
            
            // Render the separator
            let separator_matches = !self.input_buffer.is_empty() && self.input_buffer.contains('-');
            if separator_matches {
                execute!(io::stderr(), SetForegroundColor(Color::Yellow), Print("-"), ResetColor)?;
            } else {
                execute!(io::stderr(), SetForegroundColor(Color::DarkGrey), Print("-"), ResetColor)?;
            }
            
            // Render the name part with match highlighting
            if !self.input_buffer.is_empty() {
                self.print_highlighted_text(&name_part, &self.input_buffer)?;
            } else {
                execute!(io::stderr(), Print(&name_part))?;
            }
        } else {
            // No date prefix
            if !self.input_buffer.is_empty() {
                self.print_highlighted_text(&try_dir.basename, &self.input_buffer)?;
            } else {
                execute!(io::stderr(), Print(&try_dir.basename))?;
            }
        }

        // Format score and time for display
        let time_text = self.format_relative_time(&try_dir.mtime);
        let score_text = format!("{:.1}", try_dir.score);
        let meta_text = format!("{}, {}", time_text, score_text);

        // Calculate padding
        let display_text = &try_dir.basename;
        let meta_width = meta_text.len() + 1;
        let text_width = display_text.len();
        let padding_needed = (self.term_width as usize).saturating_sub(5 + text_width + meta_width).max(1);
        let padding = " ".repeat(padding_needed);

        // Print padding and metadata
        execute!(
            io::stderr(),
            Print(&padding),
            Print(" "),
            SetForegroundColor(Color::DarkGrey),
            Print(&meta_text),
            ResetColor,
        )?;

        if is_selected {
            execute!(io::stderr(), ResetColor)?;
        }

        Ok(())
    }

    fn render_create_new_option(&self, is_selected: bool) -> Result<()> {
        execute!(io::stderr(), Print("+ "))?;

        if is_selected {
            execute!(io::stderr(), SetForegroundColor(Color::Black))?;
        }

        let display_text = if self.input_buffer.is_empty() {
            "Create new".to_string()
        } else {
            format!("Create new: {}", self.input_buffer)
        };

        execute!(io::stderr(), Print(&display_text))?;

        // Pad to full width
        let text_width = display_text.len();
        let padding_needed = (self.term_width as usize).saturating_sub(5 + text_width).max(1);
        execute!(io::stderr(), Print(&" ".repeat(padding_needed)))?;

        if is_selected {
            execute!(io::stderr(), ResetColor)?;
        }

        Ok(())
    }

    fn parse_date_prefix(&self, text: &str) -> Option<(String, String)> {
        if text.len() >= 11 && text.chars().nth(4) == Some('-') && text.chars().nth(7) == Some('-') && text.chars().nth(10) == Some('-') {
            let date_part = text[..10].to_string();
            let name_part = text[11..].to_string();
            Some((date_part, name_part))
        } else {
            None
        }
    }

    fn print_highlighted_text(&self, text: &str, query: &str) -> Result<()> {
        if query.is_empty() {
            execute!(io::stderr(), Print(text))?;
            return Ok(());
        }

        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        let query_chars: Vec<char> = query_lower.chars().collect();
        let mut query_index = 0;

        for (i, ch) in text.chars().enumerate() {
            if query_index < query_chars.len() && text_lower.chars().nth(i) == Some(query_chars[query_index]) {
                execute!(io::stderr(), SetForegroundColor(Color::Yellow), Print(ch), ResetColor)?;
                query_index += 1;
            } else {
                execute!(io::stderr(), Print(ch))?;
            }
        }

        Ok(())
    }

    fn format_relative_time(&self, time: &DateTime<Utc>) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(*time);
        
        let seconds = duration.num_seconds();
        let minutes = seconds / 60;
        let hours = minutes / 60;
        let days = hours / 24;

        if seconds < 10 {
            "just now".to_string()
        } else if minutes < 60 {
            format!("{}m ago", minutes)
        } else if hours < 24 {
            format!("{}h ago", hours)
        } else if days < 30 {
            format!("{}d ago", days)
        } else if days < 365 {
            format!("{}mo ago", days / 30)
        } else {
            format!("{}y ago", days / 365)
        }
    }

    fn handle_selection(&mut self, try_dir: &TryDirectory) {
        self.selected = Some(SelectionResult {
            selection_type: SelectionType::Cd,
            path: try_dir.path.clone(),
        });
    }

    fn handle_create_new(&mut self) -> Result<()> {
        let date_prefix = chrono::Utc::now().format("%Y-%m-%d").to_string();

        if !self.input_buffer.is_empty() {
            let final_name = format!("{}-{}", date_prefix, self.input_buffer.replace(' ', "-"));
            let full_path = self.base_path.join(&final_name);
            self.selected = Some(SelectionResult {
                selection_type: SelectionType::Mkdir,
                path: full_path,
            });
        } else {
            // Prompt for name
            self.restore_terminal()?;
            
            print!("Enter new try name\n> {}-", date_prefix);
            io::stdout().flush()?;
            
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let entry = input.trim();
            
            if entry.is_empty() {
                self.selected = Some(SelectionResult {
                    selection_type: SelectionType::Cancel,
                    path: PathBuf::new(),
                });
                return Ok(());
            }

            let final_name = format!("{}-{}", date_prefix, entry.replace(' ', "-"));
            let full_path = self.base_path.join(&final_name);
            
            self.selected = Some(SelectionResult {
                selection_type: SelectionType::Mkdir,
                path: full_path,
            });
        }
        
        Ok(())
    }
}

fn get_default_try_path() -> PathBuf {
    if let Ok(try_path) = env::var("TRY_PATH") {
        PathBuf::from(try_path)
    } else if let Some(home) = home_dir() {
        home.join("src").join("tries")
    } else {
        PathBuf::from("tries")
    }
}

fn print_global_help() {
    println!("try something!");
    println!();
    println!("Lightweight experiments for people with ADHD");
    println!();
    println!("this tool is not meant to be used directly,");
    println!("but added to your ~/.zshrc or ~/.bashrc:");
    println!();
    println!("  eval \"$(try init ~/src/tries)\"");
    println!();
    println!("Usage:");
    println!("  init [--path PATH]  # Initialize shell function for aliasing");
    println!("  cd [QUERY]          # Interactive selector; prints shell cd commands");
    println!();
    println!("Defaults:");
    println!("  Default path: ~/src/tries (override with --path on commands)");
    println!("  Current default: {}", get_default_try_path().display());
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            print_global_help();
            std::process::exit(2);
        }
        Some(Commands::Init { path, tries_path }) => {
            let script_path = env::current_exe()?;
            let tries_path = path.or(tries_path).unwrap_or_else(get_default_try_path);
            let tries_path = tries_path.canonicalize().unwrap_or(tries_path);
            
            let path_arg = format!(" --path \"{}\"", tries_path.display());
            
            println!(
                r#"try() {{
  script_path='{}';
  cmd=$("$script_path" cd{} "$@" 2>/dev/tty);
  [ $? -eq 0 ] && eval "$cmd" || echo "$cmd";
}}"#,
                script_path.display(),
                path_arg
            );
        }
        Some(Commands::Cd { path, query }) => {
            let search_term = query.join(" ");
            let tries_path = path.unwrap_or_else(get_default_try_path);
            
            let mut selector = TrySelector::new(search_term, tries_path)?;
            let result = selector.run()?;

            if let Some(result) = result {
                let mut parts = Vec::new();
                parts.push(format!("dir='{}'", result.path.display()));
                
                match result.selection_type {
                    SelectionType::Mkdir => {
                        parts.push("mkdir -p \"$dir\"".to_string());
                    }
                    SelectionType::Cancel => {
                        return Ok(());
                    }
                    SelectionType::Cd => {}
                }
                
                parts.push("touch \"$dir\"".to_string());
                parts.push("cd \"$dir\"".to_string());
                
                println!("{}", parts.join(" && "));
            }
        }
    }

    Ok(())
}
