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
use regex::Regex;
use std::{
    env,
    fs::{self, Metadata},
    io::{self, Write},
    path::PathBuf,
    process::Command,
    time::UNIX_EPOCH,
};
use url::Url;

#[derive(Parser)]
#[command(name = "slop")]
#[command(about = "Vibecoding at hyperspeed - create projects OR paste GitHub URLs to clone & launch in Claude!")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize shell function for aliasing
    Init {
        /// Path to projects directory
        #[arg(long)]
        path: Option<PathBuf>,
        /// Additional path argument (for backward compatibility)
        projects_path: Option<PathBuf>,
    },
    /// Interactive project selector and creator - paste GitHub URLs to clone!
    Run {
        /// Path to projects directory
        #[arg(long)]
        path: Option<PathBuf>,
        /// Project name to create/find OR GitHub URL to clone (user/repo, github.com/user/repo, or full URL)
        query: Vec<String>,
    },
    /// Configure slop settings
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set the default projects directory
    Path {
        /// New projects directory path
        path: PathBuf,
    },
    /// Set the default editor
    Editor {
        /// Editor command (cursor, code, vim, etc.)
        editor: String,
    },
    /// Show current configuration
    Show,
    /// Reset configuration to defaults
    Reset,
}

#[derive(Debug, Clone)]
struct Project {
    name: String,
    path: PathBuf,
    last_accessed: DateTime<Utc>,
    created: DateTime<Utc>,
    score: f64,
    project_type: ProjectType,
}

#[derive(Debug, Clone)]
enum ProjectType {
    Local,
    GitRepo,
}

#[derive(Debug, Clone)]
enum ProjectTemplate {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Blank,
}

impl ProjectTemplate {
    fn get_all() -> Vec<Self> {
        vec![
            Self::Rust,
            Self::Python,
            Self::JavaScript,
            Self::TypeScript,
            Self::Go,
            Self::Blank,
        ]
    }

    fn display_name(&self) -> &str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Go => "Go",
            Self::Blank => "Blank",
        }
    }
}

struct VibeSelector {
    cursor_pos: usize,
    scroll_offset: usize,
    input_buffer: String,
    selected: Option<SelectionResult>,
    term_width: u16,
    term_height: u16,
    all_projects: Option<Vec<Project>>,
    base_path: PathBuf,
    mode: SelectorMode,
    delete_target: Option<usize>,
}

#[derive(Debug, Clone)]
enum SelectorMode {
    ProjectSelection,
    TemplateSelection,
    Configuration,
    EditingPath,
    EditingEditor,
    ConfirmDelete,
}

#[derive(Debug, Clone)]
struct SelectionResult {
    action: SelectionAction,
    path: PathBuf,
    template: Option<ProjectTemplate>,
    git_url: Option<String>,
}

#[derive(Debug, Clone)]
enum SelectionAction {
    OpenExisting,
    CreateNew,
    CloneRepo,
    Cancel,
}

impl VibeSelector {
    fn new(search_term: String, base_path: PathBuf) -> Result<Self> {
        let input_buffer = search_term.replace(' ', "-");
        
        // Create base directory if it doesn't exist
        fs::create_dir_all(&base_path)
            .with_context(|| format!("Failed to create base directory: {}", base_path.display()))?;

        let (term_width, term_height) = size().unwrap_or((80, 24));

        Ok(VibeSelector {
            cursor_pos: 0,
            scroll_offset: 0,
            input_buffer,
            selected: None,
            term_width,
            term_height,
            all_projects: None,
            base_path,
            mode: SelectorMode::ProjectSelection,
            delete_target: None,
        })
    }

    fn run(&mut self) -> Result<Option<SelectionResult>> {
        // Check if we have a TTY
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            eprintln!("Error: slop requires an interactive terminal");
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
        // Ensure minimum usable size
        self.term_width = width.max(40);
        self.term_height = height.max(10);
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

    fn load_all_projects(&mut self) -> Result<()> {
        if self.all_projects.is_some() {
            return Ok(());
        }

        let mut projects = Vec::new();
        let entries = fs::read_dir(&self.base_path)
            .with_context(|| format!("Failed to read directory: {}", self.base_path.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    let metadata = entry.metadata()?;
                    let (created, last_accessed) = self.get_times(&metadata)?;
                    
                    // Check if it's a git repo
                    let project_type = if path.join(".git").exists() {
                        ProjectType::GitRepo
                    } else {
                        ProjectType::Local
                    };
                    
                    projects.push(Project {
                        name: name.to_string(),
                        path: path.clone(),
                        last_accessed,
                        created,
                        score: 0.0,
                        project_type,
                    });
                }
            }
        }

        self.all_projects = Some(projects);
        Ok(())
    }


    fn get_times(&self, metadata: &Metadata) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
        let created = metadata
            .created()
            .or_else(|_| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        let last_accessed = metadata.modified().unwrap_or(UNIX_EPOCH);
        
        let created = DateTime::from(created);
        let last_accessed = DateTime::from(last_accessed);
        
        Ok((created, last_accessed))
    }

    fn get_projects(&mut self) -> Result<Vec<Project>> {
        self.load_all_projects()?;
        
        let mut scored_projects: Vec<Project> = self
            .all_projects
            .as_ref()
            .unwrap()
            .iter()
            .map(|project| {
                let score = self.calculate_score(
                    &project.name,
                    &self.input_buffer,
                    &project.created,
                    &project.last_accessed,
                );
                let mut project = project.clone();
                project.score = score;
                project
            })
            .collect();

        // Filter and sort
        if self.input_buffer.is_empty() {
            scored_projects.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        } else {
            scored_projects.retain(|p| p.score > 0.0);
            scored_projects.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        }

        Ok(scored_projects)
    }

    fn calculate_score(&self, text: &str, query: &str, created: &DateTime<Utc>, last_accessed: &DateTime<Utc>) -> f64 {
        let mut score = 0.0;

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
        let days_old = (now - *created).num_seconds() as f64 / 86400.0;
        score += 2.0 / (days_old + 1.0).sqrt();

        // Access time bonus (most important)
        let hours_since_access = (now - *last_accessed).num_seconds() as f64 / 3600.0;
        score += 5.0 / (hours_since_access + 1.0).sqrt();

        score
    }

    fn main_loop(&mut self) -> Result<Option<SelectionResult>> {
        loop {
            match self.mode {
                SelectorMode::ProjectSelection => {
                    let projects = self.get_projects()?;
                    
                    let create_new_text = if self.input_buffer.is_empty() {
                        "✨ Create new project (select template)".to_string()
                    } else if self.is_github_url(&self.input_buffer) {
                        let repo_name = self.extract_repo_name(&self.normalize_github_url(&self.input_buffer));
                        format!("🚀 Clone {}", repo_name)
                    } else {
                        format!("✨ Create {} (blank template)", self.input_buffer)
                    };

                    let total_items = projects.len() + 2; // +1 for create new, +1 for config

                    // Ensure cursor is within bounds
                    self.cursor_pos = self.cursor_pos.min(total_items.saturating_sub(1));

                    self.render_project_selection(&projects, &create_new_text)?;

                    // Update terminal size before handling input
                    self.update_terminal_size()?;
                    
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
                            KeyEvent { code: KeyCode::Enter, .. } => {
                                if self.cursor_pos < projects.len() {
                                    // Selected existing project
                                    self.handle_project_selection(&projects[self.cursor_pos]);
                                } else if self.cursor_pos == projects.len() {
                                    // Selected "Create new"
                                    if self.is_github_url(&self.input_buffer) {
                                        self.handle_clone_repo()?;
                                    } else if !self.input_buffer.is_empty() {
                                        // If name is already typed, create with default template
                                        self.handle_template_selection(ProjectTemplate::Blank)?;
                                    } else {
                                        // No name typed, go to template selection
                                        self.handle_create_new()?;
                                    }
                                } else {
                                    // Selected "Configure"
                                    self.mode = SelectorMode::Configuration;
                                    self.cursor_pos = 0;
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
                                if !self.input_buffer.is_empty() {
                                    self.input_buffer.clear();
                                    self.cursor_pos = 0;
                                } else {
                                    self.selected = None;
                                    break;
                                }
                            }
                            KeyEvent { code: KeyCode::Delete, .. } | KeyEvent { code: KeyCode::Char('d'), .. } => {
                                if self.cursor_pos < projects.len() {
                                    self.delete_target = Some(self.cursor_pos);
                                    self.mode = SelectorMode::ConfirmDelete;
                                }
                            }
                            KeyEvent { code: KeyCode::Char(ch), .. } => {
                                if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ' ' || ch == '/' || ch == ':' {
                                    self.input_buffer.push(ch);
                                    self.cursor_pos = 0;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                SelectorMode::TemplateSelection => {
                    let templates = ProjectTemplate::get_all();
                    self.cursor_pos = self.cursor_pos.min(templates.len().saturating_sub(1));
                    
                    self.render_template_selection(&templates)?;

                    if let Event::Key(key) = event::read()? {
                        match key {
                            KeyEvent { code: KeyCode::Up, .. } | KeyEvent { code: KeyCode::Char('p'), modifiers: KeyModifiers::CONTROL, .. } => {
                                if self.cursor_pos > 0 {
                                    self.cursor_pos -= 1;
                                }
                            }
                            KeyEvent { code: KeyCode::Down, .. } | KeyEvent { code: KeyCode::Char('n'), modifiers: KeyModifiers::CONTROL, .. } => {
                                if self.cursor_pos < templates.len().saturating_sub(1) {
                                    self.cursor_pos += 1;
                                }
                            }
                            KeyEvent { code: KeyCode::Enter, .. } => {
                                let template = templates[self.cursor_pos].clone();
                                self.handle_template_selection(template)?;
                                break;
                            }
                            KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. } | 
                            KeyEvent { code: KeyCode::Esc, .. } => {
                                self.mode = SelectorMode::ProjectSelection;
                                self.cursor_pos = 0;
                            }
                            KeyEvent { code: KeyCode::Backspace, .. } => {
                                if !self.input_buffer.is_empty() {
                                    self.input_buffer.pop();
                                }
                            }
                            KeyEvent { code: KeyCode::Char(ch), .. } => {
                                if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ' ' {
                                    self.input_buffer.push(ch);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                SelectorMode::Configuration => {
                    self.render_configuration_interface()?;

                    if let Event::Key(key) = event::read()? {
                        match key {
                            KeyEvent { code: KeyCode::Up, .. } | KeyEvent { code: KeyCode::Char('p'), modifiers: KeyModifiers::CONTROL, .. } => {
                                if self.cursor_pos > 0 {
                                    self.cursor_pos -= 1;
                                }
                            }
                            KeyEvent { code: KeyCode::Down, .. } | KeyEvent { code: KeyCode::Char('n'), modifiers: KeyModifiers::CONTROL, .. } => {
                                if self.cursor_pos < 2 { // 3 options: path, editor, back
                                    self.cursor_pos += 1;
                                }
                            }
                            KeyEvent { code: KeyCode::Enter, .. } => {
                                match self.cursor_pos {
                                    0 => {
                                        self.mode = SelectorMode::EditingPath;
                                        let config = load_config(&get_config_file_path()?).unwrap_or_default();
                                        self.input_buffer = config.projects_path.display().to_string();
                                    },
                                    1 => {
                                        self.mode = SelectorMode::EditingEditor;
                                        let config = load_config(&get_config_file_path()?).unwrap_or_default();
                                        self.input_buffer = config.default_editor.clone();
                                    },
                                    _ => {
                                        self.mode = SelectorMode::ProjectSelection;
                                        self.cursor_pos = 0;
                                    }
                                }
                            }
                            KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. } | 
                            KeyEvent { code: KeyCode::Esc, .. } => {
                                self.mode = SelectorMode::ProjectSelection;
                                self.cursor_pos = 0;
                            }
                            _ => {}
                        }
                    }
                }
                SelectorMode::EditingPath => {
                    self.render_inline_edit("📁 Projects Path", &self.input_buffer.clone())?;
                    
                    if let Event::Key(key) = event::read()? {
                        match key {
                            KeyEvent { code: KeyCode::Enter, .. } => {
                                let mut config = load_config(&get_config_file_path()?).unwrap_or_default();
                                config.projects_path = PathBuf::from(&self.input_buffer);
                                save_config(&config)?;
                                self.mode = SelectorMode::Configuration;
                                self.input_buffer.clear();
                            }
                            KeyEvent { code: KeyCode::Esc, .. } => {
                                self.mode = SelectorMode::Configuration;
                                self.input_buffer.clear();
                            }
                            KeyEvent { code: KeyCode::Backspace, .. } => {
                                self.input_buffer.pop();
                            }
                            KeyEvent { code: KeyCode::Char(c), .. } => {
                                self.input_buffer.push(c);
                            }
                            _ => {}
                        }
                    }
                }
                SelectorMode::EditingEditor => {
                    self.render_inline_edit("✏️  Editor Command", &self.input_buffer.clone())?;
                    
                    if let Event::Key(key) = event::read()? {
                        match key {
                            KeyEvent { code: KeyCode::Enter, .. } => {
                                let mut config = load_config(&get_config_file_path()?).unwrap_or_default();
                                config.default_editor = self.input_buffer.clone();
                                save_config(&config)?;
                                self.mode = SelectorMode::Configuration;
                                self.input_buffer.clear();
                            }
                            KeyEvent { code: KeyCode::Esc, .. } => {
                                self.mode = SelectorMode::Configuration;
                                self.input_buffer.clear();
                            }
                            KeyEvent { code: KeyCode::Backspace, .. } => {
                                self.input_buffer.pop();
                            }
                            KeyEvent { code: KeyCode::Char(c), .. } => {
                                self.input_buffer.push(c);
                            }
                            _ => {}
                        }
                    }
                }
                SelectorMode::ConfirmDelete => {
                    if let Some(delete_idx) = self.delete_target {
                        let projects = self.get_projects()?;
                        if delete_idx < projects.len() {
                            let project = &projects[delete_idx];
                            self.render_delete_confirmation(project)?;
                            
                            if let Event::Key(key) = event::read()? {
                                match key {
                                    KeyEvent { code: KeyCode::Char('y'), .. } | KeyEvent { code: KeyCode::Char('Y'), .. } => {
                                        self.delete_project(project)?;
                                        self.all_projects = None; // Force reload
                                        self.mode = SelectorMode::ProjectSelection;
                                        self.delete_target = None;
                                        self.cursor_pos = 0;
                                    }
                                    _ => {
                                        self.mode = SelectorMode::ProjectSelection;
                                        self.delete_target = None;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(self.selected.clone())
    }

    fn is_github_url(&self, input: &str) -> bool {
        if let Ok(url) = Url::parse(input) {
            url.host_str() == Some("github.com")
        } else {
            // Also accept github.com/user/repo format and user/repo shorthand
            let github_regex = Regex::new(r"^(github\.com/)?[\w\-\.]+/[\w\-\.]+(/.*)?$").unwrap();
            github_regex.is_match(input) && !input.contains(' ')
        }
    }

    fn render_project_selection(&mut self, projects: &[Project], create_new_text: &str) -> Result<()> {
        execute!(io::stderr(), Clear(ClearType::All), MoveTo(0, 0))?;

        let separator = "─".repeat(self.term_width.saturating_sub(1).max(10) as usize);

        // Header
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Cyan),
            Print("slop"),
            ResetColor,
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Search input
        if self.input_buffer.is_empty() {
            execute!(
                io::stderr(),
                SetForegroundColor(Color::DarkGrey),
                Print("Search or paste GitHub URL"),
                ResetColor,
                Print("\r\n"),
            )?;
        } else if self.is_github_url(&self.input_buffer) {
            execute!(
                io::stderr(),
                SetForegroundColor(Color::Green),
                Print("🌐 "),
                Print(&self.input_buffer),
                ResetColor,
                Print("\r\n"),
            )?;
        } else {
            execute!(
                io::stderr(),
                Print(&self.input_buffer),
                Print("\r\n"),
            )?;
        }
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Calculate visible window
        let max_visible = (self.term_height as usize).saturating_sub(8).max(3);
        let total_items = projects.len() + 2; // +1 for create new, +1 for config

        // Adjust scroll window
        if self.cursor_pos < self.scroll_offset {
            self.scroll_offset = self.cursor_pos;
        } else if self.cursor_pos >= self.scroll_offset + max_visible {
            self.scroll_offset = self.cursor_pos.saturating_sub(max_visible - 1);
        }

        // Display items
        let visible_end = (self.scroll_offset + max_visible).min(total_items);

        for idx in self.scroll_offset..visible_end {
            let is_selected = idx == self.cursor_pos;
            
            // Better selection highlighting
            if is_selected {
                execute!(
                    io::stderr(),
                    SetForegroundColor(Color::Yellow),
                    Print("▶ "),
                    ResetColor
                )?;
            } else {
                execute!(io::stderr(), Print("  "))?;
            }

            if idx < projects.len() {
                let project = &projects[idx];
                self.render_project(project, is_selected)?;
            } else if idx == projects.len() {
                // Create new option
                if is_selected {
                    execute!(
                        io::stderr(),
                        SetForegroundColor(Color::Yellow),
                        Print(&create_new_text),
                        ResetColor
                    )?;
                } else {
                    execute!(io::stderr(), Print(&create_new_text))?;
                }
            } else {
                // Configuration option
                if is_selected {
                    execute!(
                        io::stderr(),
                        SetForegroundColor(Color::Yellow),
                        Print("⚙️  Configure"),
                        ResetColor
                    )?;
                } else {
                    execute!(io::stderr(), Print("⚙️  Configure"))?;
                }
            }

            execute!(io::stderr(), Print("\r\n"))?;
        }


        // Instructions at bottom
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            Print("\r\n"),
            Print("Type: Project name  ↑↓: Navigate  Enter: Select  D: Delete  ESC: Clear"),
            ResetColor,
        )?;

        io::stderr().flush()?;
        Ok(())
    }

    fn render_template_selection(&mut self, templates: &[ProjectTemplate]) -> Result<()> {
        execute!(io::stderr(), Clear(ClearType::All), MoveTo(0, 0))?;

        let separator = "─".repeat(self.term_width.saturating_sub(1).max(10) as usize);

        // Header
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Cyan),
            Print("✨ Choose Project Template"),
            ResetColor,
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Show project name being created
        if !self.input_buffer.is_empty() {
            execute!(
                io::stderr(),
                SetForegroundColor(Color::Green),
                Print("Creating: "),
                Print(&self.input_buffer),
                ResetColor,
                Print("\r\n"),
            )?;
        } else {
            execute!(
                io::stderr(),
                SetForegroundColor(Color::DarkGrey),
                Print("Creating: new-project"),
                ResetColor,
                Print("\r\n"),
            )?;
        }
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        for (idx, template) in templates.iter().enumerate() {
            let is_selected = idx == self.cursor_pos;
            if is_selected {
                execute!(io::stderr(), SetForegroundColor(Color::Yellow), Print("→ "), ResetColor)?;
            } else {
                execute!(io::stderr(), Print("  "))?;
            }

            execute!(io::stderr(), Print(template.display_name()), Print("\r\n"))?;
        }

        // Instructions at bottom
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            Print("\r\n"),
            Print("↑↓: Navigate  Enter: Select  Type: Edit name  ESC: Back"),
            ResetColor,
        )?;

        io::stderr().flush()?;
        Ok(())
    }

    fn render_configuration_interface(&mut self) -> Result<()> {
        execute!(io::stderr(), Clear(ClearType::All), MoveTo(0, 0))?;

        let separator = "─".repeat(self.term_width.saturating_sub(1).max(10) as usize);

        // Header - match main UI style
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Cyan),
            Print("⚙️  Configuration"),
            ResetColor,
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Load current config - ensure defaults if file doesn't exist
        let config = load_config(&get_config_file_path()?).unwrap_or_else(|_| {
            let default_config = VibeConfig::default();
            save_config(&default_config).ok();
            default_config
        });

        // Configuration options
        let options = [
            ("📁 Projects Path", config.projects_path.display().to_string()),
            ("✏️  Editor", config.default_editor.clone()),
            ("← Back", String::new()),
        ];

        for (idx, (label, value)) in options.iter().enumerate() {
            let is_selected = idx == self.cursor_pos;
            
            // Match main UI selection style
            if is_selected {
                execute!(
                    io::stderr(),
                    SetForegroundColor(Color::Yellow),
                    Print("▶ "),
                    ResetColor
                )?;
            } else {
                execute!(io::stderr(), Print("  "))?;
            }

            // Apply selection highlighting consistently
            if is_selected {
                execute!(
                    io::stderr(),
                    SetForegroundColor(Color::Yellow),
                    Print(label),
                    ResetColor,
                )?;
            } else {
                execute!(io::stderr(), Print(label))?;
            }
            
            if !value.is_empty() {
                execute!(
                    io::stderr(),
                    Print(": "),
                    SetForegroundColor(Color::Cyan),
                    Print(value),
                    ResetColor,
                )?;
            }
            execute!(io::stderr(), Print("\r\n"))?;
        }

        // Instructions at bottom
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            Print("\r\n"),
            Print("↑↓: Navigate  Enter: Edit  ESC: Back"),
            ResetColor,
        )?;

        io::stderr().flush()?;
        Ok(())
    }

    fn render_inline_edit(&self, label: &str, value: &str) -> Result<()> {
        execute!(io::stderr(), Clear(ClearType::All), MoveTo(0, 0))?;

        let separator = "─".repeat(self.term_width.saturating_sub(1).max(10) as usize);

        // Header - match main UI style
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Cyan),
            Print("⚙️  Configuration"),
            ResetColor,
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Edit field with consistent selection highlighting
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Yellow),
            Print("▶ "),
            Print(label),
            Print(": "),
            Print(value),
            Print("█"), // cursor
            ResetColor,
            Print("\r\n"),
        )?;

        // Instructions - match main UI style
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            Print("\r\n"),
            Print("Type to edit  Enter: Save  ESC: Cancel"),
            ResetColor,
        )?;

        io::stderr().flush()?;
        Ok(())
    }

    fn render_delete_confirmation(&self, project: &Project) -> Result<()> {
        execute!(io::stderr(), Clear(ClearType::All), MoveTo(0, 0))?;

        let separator = "─".repeat(self.term_width.saturating_sub(1).max(10) as usize);

        // Header
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Red),
            Print("🗑️  Delete Project"),
            ResetColor,
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            ResetColor,
            Print("\r\n"),
        )?;

        // Warning
        execute!(
            io::stderr(),
            SetForegroundColor(Color::Yellow),
            Print("⚠️  Delete "),
            SetForegroundColor(Color::Cyan),
            Print(&project.name),
            SetForegroundColor(Color::Yellow),
            Print("?"),
            ResetColor,
            Print("\r\n"),
            SetForegroundColor(Color::DarkGrey),
            Print(&format!("   {}", project.path.display())),
            ResetColor,
            Print("\r\n"),
            Print("\r\n"),
            SetForegroundColor(Color::Red),
            Print("This will permanently delete the entire folder!"),
            ResetColor,
            Print("\r\n"),
        )?;

        // Instructions
        execute!(
            io::stderr(),
            SetForegroundColor(Color::DarkGrey),
            Print(&separator),
            Print("\r\n"),
            Print("Y: Delete  Any other key: Cancel"),
            ResetColor,
        )?;

        io::stderr().flush()?;
        Ok(())
    }

    fn delete_project(&self, project: &Project) -> Result<()> {
        fs::remove_dir_all(&project.path)
            .with_context(|| format!("Failed to delete project: {}", project.path.display()))?;
        Ok(())
    }

    fn render_project(&self, project: &Project, is_selected: bool) -> Result<()> {
        // Project type icon
        let icon = match project.project_type {
            ProjectType::Local => "📁",
            ProjectType::GitRepo => "🌐",
        };

        execute!(io::stderr(), Print(format!("{} ", icon)))?;

        // Project name with better color handling
        if is_selected {
            execute!(
                io::stderr(),
                SetForegroundColor(Color::Yellow),
                Print(&project.name),
                ResetColor,
            )?;
        } else {
            execute!(io::stderr(), Print(&project.name))?;
        }

        // Format metadata
        let time_text = self.format_relative_time(&project.last_accessed);
        let score_text = format!("{:.1}", project.score);
        let meta_text = format!("{}, {}", time_text, score_text);

        // Calculate padding - handle small terminals gracefully
        let text_width = project.name.len();
        let meta_width = meta_text.len() + 1;
        let min_width = 5 + text_width + meta_width;
        
        if (self.term_width as usize) >= min_width {
            let padding_needed = (self.term_width as usize).saturating_sub(min_width).max(1);
            let padding = " ".repeat(padding_needed);
            execute!(
                io::stderr(),
                Print(&padding),
                Print(" "),
                SetForegroundColor(Color::DarkGrey),
                Print(&meta_text),
                ResetColor,
            )?;
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
            format!("{}m", minutes)
        } else if hours < 24 {
            format!("{}h", hours)
        } else if days < 30 {
            format!("{}d", days)
        } else if days < 365 {
            format!("{}mo", days / 30)
        } else {
            format!("{}y", days / 365)
        }
    }

    fn handle_project_selection(&mut self, project: &Project) {
        self.selected = Some(SelectionResult {
            action: SelectionAction::OpenExisting,
            path: project.path.clone(),
            template: None,
            git_url: None,
        });
    }

    fn handle_create_new(&mut self) -> Result<()> {
        self.mode = SelectorMode::TemplateSelection;
        self.cursor_pos = 0;
        Ok(())
    }

    fn handle_clone_repo(&mut self) -> Result<()> {
        let url = self.normalize_github_url(&self.input_buffer);
        let repo_name = self.extract_repo_name(&url);
        let project_path = self.base_path.join(&repo_name);
        
        self.selected = Some(SelectionResult {
            action: SelectionAction::CloneRepo,
            path: project_path,
            template: None,
            git_url: Some(url),
        });
        
        Ok(())
    }

    fn handle_template_selection(&mut self, template: ProjectTemplate) -> Result<()> {
        let project_name = if self.input_buffer.is_empty() {
            // If no name was entered, use a default name
            "new-project".to_string()
        } else {
            self.input_buffer.clone()
        };

        let project_path = self.base_path.join(&project_name.replace(' ', "-"));
        
        self.selected = Some(SelectionResult {
            action: SelectionAction::CreateNew,
            path: project_path,
            template: Some(template),
            git_url: None,
        });
        
        Ok(())
    }

    fn normalize_github_url(&self, input: &str) -> String {
        if input.starts_with("http") {
            input.to_string()
        } else if input.starts_with("github.com/") {
            format!("https://{}", input)
        } else {
            format!("https://github.com/{}", input)
        }
    }

    fn extract_repo_name(&self, url: &str) -> String {
        if let Ok(parsed_url) = Url::parse(url) {
            let path = parsed_url.path();
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            if parts.len() >= 2 {
                let repo_name = parts[1].trim_end_matches(".git");
                return repo_name.to_string();
            }
        }
        
        // Fallback: extract from the end
        url.split('/').last().unwrap_or("unknown-repo").trim_end_matches(".git").to_string()
    }

}

fn get_default_projects_path() -> PathBuf {
    // Check environment variable first
    if let Ok(projects_path) = env::var("slop_PATH") {
        return PathBuf::from(projects_path);
    }
    
    // Check config file
    if let Ok(config_path) = get_config_file_path() {
        if let Ok(config) = load_config(&config_path) {
            if !config.projects_path.as_os_str().is_empty() {
                return config.projects_path;
            }
        }
    }
    
    // Default fallback
    if let Some(home) = home_dir() {
        home.join("src").join("slop")
    } else {
        PathBuf::from("slop")
    }
}

#[derive(Debug, Clone)]
struct VibeConfig {
    projects_path: PathBuf,
    default_editor: String,
}

impl Default for VibeConfig {
    fn default() -> Self {
        let default_path = if let Some(home) = home_dir() {
            home.join("src").join("slop")
        } else {
            PathBuf::from("slop")
        };
        
        Self {
            projects_path: default_path,
            default_editor: "claude".to_string(),
        }
    }
}

fn get_config_file_path() -> Result<PathBuf> {
    if let Some(home) = home_dir() {
        Ok(home.join(".config").join("slop").join("config.toml"))
    } else {
        Err(anyhow::anyhow!("Could not find home directory"))
    }
}

fn load_config(config_path: &PathBuf) -> Result<VibeConfig> {
    if !config_path.exists() {
        return Ok(VibeConfig::default());
    }
    
    let content = fs::read_to_string(config_path)?;
    let mut config = VibeConfig::default();
    
    // Simple TOML-like parsing (we could use a proper TOML crate, but keeping dependencies minimal)
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            
            match key {
                "projects_path" => {
                    config.projects_path = PathBuf::from(value);
                }
                "default_editor" => {
                    config.default_editor = value.to_string();
                }
                _ => {} // Ignore unknown keys
            }
        }
    }
    
    Ok(config)
}

fn save_config(config: &VibeConfig) -> Result<()> {
    let config_path = get_config_file_path()?;
    
    // Create config directory if it doesn't exist
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let content = format!(
        r#"# slop Configuration
# Path where projects are stored
projects_path = "{}"

# Default editor to open projects (cursor, code, etc.)
default_editor = "{}"
"#,
        config.projects_path.display(),
        config.default_editor
    );
    
    fs::write(&config_path, content)?;
    Ok(())
}

fn print_global_help() {
    // Load config to show current editor
    let config = load_config(&get_config_file_path().unwrap_or_default()).unwrap_or_default();
    
    println!("slop - vibecoding at hyperspeed");
    println!();
    println!("Create projects, clone repos, and launch into {} instantly", config.default_editor);
    println!();
    println!("Setup (add to ~/.zshrc or ~/.bashrc):");
    println!("  eval \"$(slop init ~/src/slop)\"");
    println!();
    println!("Usage:");
    println!("  slop                             # Browse and create projects");
    println!("  slop my-cool-app                 # Create or find 'my-cool-app'");
    println!();
    println!("🌐 GitHub Integration - Just paste any GitHub URL:");
    println!("  slop https://github.com/user/repo     # Clone full URL");
    println!("  slop github.com/user/repo             # Clone without https");
    println!("  slop user/repo                        # Clone shorthand");
    println!("  slop openAI/GPT-5                   # Example: clone GPT-5");
    println!();
    println!("Configuration:");
    println!("  slop config show          # Show current settings");
    println!("  slop config path <PATH>   # Set projects directory");
    println!("  slop config editor <CMD>  # Set editor command (claude, cursor, code)");
    println!();
    println!("Default path: ~/src/slop");
    println!("Current path: {}", get_default_projects_path().display());
    if let Ok(config_path) = get_config_file_path() {
        println!("Config file:  {}", config_path.display());
    }
}

fn create_project_from_template(path: &PathBuf, template: &ProjectTemplate) -> Result<()> {
    fs::create_dir_all(path)?;
    
    match template {
        ProjectTemplate::Rust => {
            // Create Cargo.toml
            let cargo_toml = format!(
                r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
"#,
                path.file_name().unwrap().to_string_lossy()
            );
            fs::write(path.join("Cargo.toml"), cargo_toml)?;
            
            // Create src/main.rs
            fs::create_dir_all(path.join("src"))?;
            fs::write(
                path.join("src/main.rs"),
                "fn main() {\n    println!(\"Hello, world!\");\n}\n"
            )?;
        },
        ProjectTemplate::Python => {
            fs::write(path.join("main.py"), "#!/usr/bin/env python3\n\nif __name__ == \"__main__\":\n    print(\"Hello, world!\")\n")?;
            fs::write(path.join("requirements.txt"), "")?;
        },
        ProjectTemplate::JavaScript => {
            let package_json = format!(
                r#"{{
  "name": "{}",
  "version": "1.0.0",
  "description": "",
  "main": "index.js",
  "scripts": {{
    "start": "node index.js"
  }},
  "dependencies": {{}}
}}
"#,
                path.file_name().unwrap().to_string_lossy()
            );
            fs::write(path.join("package.json"), package_json)?;
            fs::write(path.join("index.js"), "console.log('Hello, world!');\n")?;
        },
        ProjectTemplate::TypeScript => {
            let package_json = format!(
                r#"{{
  "name": "{}",
  "version": "1.0.0",
  "description": "",
  "main": "dist/index.js",
  "scripts": {{
    "build": "tsc",
    "start": "node dist/index.js",
    "dev": "ts-node src/index.ts"
  }},
  "devDependencies": {{
    "typescript": "^5.0.0",
    "@types/node": "^20.0.0",
    "ts-node": "^10.0.0"
  }}
}}
"#,
                path.file_name().unwrap().to_string_lossy()
            );
            fs::write(path.join("package.json"), package_json)?;
            
            let tsconfig = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "outDir": "./dist",
    "rootDir": "./src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  }
}
"#;
            fs::write(path.join("tsconfig.json"), tsconfig)?;
            
            fs::create_dir_all(path.join("src"))?;
            fs::write(path.join("src/index.ts"), "console.log('Hello, world!');\n")?;
        },
        ProjectTemplate::Go => {
            let go_mod = format!("module {}\n\ngo 1.21\n", path.file_name().unwrap().to_string_lossy());
            fs::write(path.join("go.mod"), go_mod)?;
            fs::write(path.join("main.go"), "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"Hello, world!\")\n}\n")?;
        },
        ProjectTemplate::Blank => {
            // Just create a README
            fs::write(path.join("README.md"), format!("# {}\n\n", path.file_name().unwrap().to_string_lossy()))?;
        },
    }
    
    Ok(())
}

fn clone_repository(url: &str, path: &PathBuf) -> Result<()> {
    let output = Command::new("git")
        .arg("clone")
        .arg(url)
        .arg(path)
        .output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Git clone failed: {}", error));
    }

    Ok(())
}

fn open_in_editor(path: &PathBuf, config: &VibeConfig) -> Result<()> {
    // Change to project directory first
    env::set_current_dir(path)?;
    
    // Try configured editor first, then fallbacks
    let mut editors_to_try = vec![config.default_editor.as_str()];
    
    // Add fallbacks if they're not already the default
    if config.default_editor != "claude" {
        editors_to_try.push("claude");
    }
    if config.default_editor != "cursor" {
        editors_to_try.push("cursor");
    }
    if config.default_editor != "code" {
        editors_to_try.push("code");
    }
    
    for editor in &editors_to_try {
        let child = Command::new(editor)
            .arg(".")
            .spawn();
            
        if let Ok(mut process) = child {
            println!("🚀 Opening in {}...", editor);
            
            // Wait for the editor to close
            let _ = process.wait();
            
            // Capture quick notes
            capture_quick_notes(path)?;
            
            // Return to slop navigator
            let current_exe = env::current_exe()?;
            let mut new_process = Command::new(current_exe)
                .arg("run")
                .arg("--path")
                .arg(path.parent().unwrap_or(path))
                .spawn()?;
            
            let _ = new_process.wait();
            return Ok(());
        }
    }
    
    eprintln!("⚠️  Could not find {} in PATH", config.default_editor);
    println!("📁 Project at: {}", path.display());
    Ok(())
}

fn capture_quick_notes(project_path: &PathBuf) -> Result<()> {
    println!();
    println!("💭 Quick thoughts about this session? (Enter to skip)");
    print!("> ");
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let notes = input.trim();
    
    if !notes.is_empty() {
        save_notes_to_project(project_path, notes)?;
        println!("✅ Notes saved to project");
    }
    
    Ok(())
}

fn save_notes_to_project(project_path: &PathBuf, notes: &str) -> Result<()> {
    let notes_file = project_path.join("NOTES.md");
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    
    let note_entry = format!("\n## {}\n{}\n", timestamp, notes);
    
    // Append to existing notes or create new file
    if notes_file.exists() {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&notes_file)?;
        write!(file, "{}", note_entry)?;
    } else {
        let header = format!("# Notes\n{}", note_entry);
        fs::write(&notes_file, header)?;
    }
    
    Ok(())
}

fn update_access_time(path: &PathBuf) -> Result<()> {
    // Touch a hidden file to update access time
    let access_file = path.join(".slop_access");
    fs::write(&access_file, "")?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            print_global_help();
            std::process::exit(2);
        }
        Some(Commands::Init { path, projects_path }) => {
            let script_path = env::current_exe()?;
            let projects_path = path.or(projects_path).unwrap_or_else(get_default_projects_path);
            let projects_path = projects_path.canonicalize().unwrap_or(projects_path);
            
            let path_arg = format!(" --path \"{}\"", projects_path.display());
            
            println!(
                r#"slop() {{
  script_path='{}';
  
  # Handle special commands that should not be executed
  if [ $# -eq 0 ]; then
    # No arguments - run interactive mode
    "$script_path" run{} 2>/dev/tty;
  else
    case "$1" in
      --help|-h|help|config|init)
        # Pass these commands directly to slop
        "$script_path" "$@"
        ;;
      *)
        # For everything else, use run command
        "$script_path" run{} "$@" 2>/dev/tty;
        ;;
    esac
  fi
}}"#,
                script_path.display(),
                path_arg,
                path_arg
            );
        }
        Some(Commands::Config { action }) => {
            match action {
                None => {
                    println!("📝 slop Configuration");
                    println!();
                    println!("Available commands:");
                    println!("  slop config show                    # Show current config");
                    println!("  slop config path <PATH>             # Set projects directory");
                    println!("  slop config editor <COMMAND>        # Set editor command");
                    println!("  slop config reset                   # Reset to defaults");
                    println!();
                    println!("Examples:");
                    println!("  slop config editor claude           # Use Claude");
                    println!("  slop config editor cursor           # Use Cursor");
                    println!("  slop config editor \"code --wait\"    # VS Code with flags");
                    println!("  slop config editor nvim             # Neovim");
                    println!("  slop config path ~/dev/projects     # Custom projects path");
                }
                Some(ConfigAction::Show) => {
                    let config = load_config(&get_config_file_path()?).unwrap_or_default();
                    println!("📝 Configuration");
                    println!();
                    println!("Projects Path: {}", config.projects_path.display());
                    println!("Editor:        {}", config.default_editor);
                    println!();
                    println!("Config file: {}", get_config_file_path()?.display());
                }
                Some(ConfigAction::Path { path }) => {
                    let mut config = load_config(&get_config_file_path()?).unwrap_or_default();
                    config.projects_path = path.clone();
                    save_config(&config)?;
                    println!("✅ Projects path set to: {}", path.display());
                }
                Some(ConfigAction::Editor { editor }) => {
                    let mut config = load_config(&get_config_file_path()?).unwrap_or_default();
                    config.default_editor = editor.clone();
                    save_config(&config)?;
                    println!("✅ Default editor set to: {}", editor);
                }
                Some(ConfigAction::Reset) => {
                    let config = VibeConfig::default();
                    save_config(&config)?;
                    println!("✅ Reset to defaults");
                    println!("Projects Path: {}", config.projects_path.display());
                    println!("Editor:        {}", config.default_editor);
                }
            }
        }
        Some(Commands::Run { path, query }) => {
            let search_term = query.join(" ");
            let projects_path = path.unwrap_or_else(get_default_projects_path);
            let config = load_config(&get_config_file_path()?).unwrap_or_default();
            
            let mut selector = VibeSelector::new(search_term, projects_path)?;
            let result = selector.run()?;

            if let Some(result) = result {
                match result.action {
                    SelectionAction::OpenExisting => {
                        update_access_time(&result.path)?;
                        open_in_editor(&result.path, &config)?;
                    }
                    SelectionAction::CreateNew => {
                        if let Some(template) = result.template {
                            create_project_from_template(&result.path, &template)?;
                            update_access_time(&result.path)?;
                            open_in_editor(&result.path, &config)?;
                        }
                    }
                    SelectionAction::CloneRepo => {
                        if let Some(url) = result.git_url {
                            println!("🌐 Cloning {}...", url);
                            clone_repository(&url, &result.path)?;
                            update_access_time(&result.path)?;
                            open_in_editor(&result.path, &config)?;
                        }
                    }
                    SelectionAction::Cancel => {
                        println!("Cancelled.");
                    }
                }
            }
        }
    }

    Ok(())
}