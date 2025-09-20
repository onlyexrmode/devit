use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor::Show, execute};
use ratatui::backend::{Backend, CrosstermBackend, TestBackend};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;
use serde::Deserialize;

const DEFAULT_MAX_EVENTS: usize = 100;
const MAX_EVENT_DETAIL_BYTES: usize = 4096;

#[derive(Parser, Debug, Clone)]
#[command(name = "devit-tui", version, about = "DevIt TUI: timeline + statusbar")]
struct Args {
    /// Path to journal JSONL (e.g., .devit/journal.jsonl)
    #[arg(long = "journal-path", value_name = "PATH")]
    journal_path: Option<PathBuf>,

    /// Follow new lines appended to journal
    #[arg(long, default_value_t = false)]
    follow: bool,

    /// Open a unified diff (path or '-' for stdin)
    #[arg(long = "open", alias = "open-diff", value_name = "PATH")]
    open_target: Option<PathBuf>,

    /// Open a journal log (path or '-' for stdin)
    #[arg(long = "open-log", value_name = "PATH")]
    open_log: Option<PathBuf>,

    /// Limit timeline to the last N events (default 100)
    #[arg(long = "seek-last", value_name = "N")]
    seek_last: Option<usize>,

    /// List available recipes as JSON (headless helper)
    #[arg(long = "list-recipes", default_value_t = false)]
    list_recipes: bool,

    /// Run a recipe by id (optionally with --dry-run)
    #[arg(long = "run-recipe", value_name = "ID")]
    run_recipe: Option<String>,

    /// Perform a dry-run for --run-recipe (no changes, preview diff)
    #[arg(long = "dry-run", default_value_t = false)]
    dry_run: bool,
}

struct App {
    lines: Vec<String>,
    selected: usize,
    follow: bool,
    journal_path: Option<PathBuf>,
    last_size: u64,
    base_status: String,
    status: String,
    help: bool,
    diff: Option<DiffState>,
    recipes: RecipeState,
    max_events: usize,
}

impl App {
    fn new(
        journal_path: Option<PathBuf>,
        follow: bool,
        base_status: String,
        max_events: usize,
    ) -> Self {
        Self {
            lines: Vec::new(),
            selected: 0,
            follow,
            journal_path,
            last_size: 0,
            status: base_status.clone(),
            base_status,
            help: false,
            diff: None,
            recipes: RecipeState::default(),
            max_events: max_events.max(1),
        }
    }

    fn load_initial(&mut self, seek_last: Option<usize>) -> Result<()> {
        let Some(p) = &self.journal_path else {
            return Ok(());
        };
        let meta =
            fs::metadata(p).with_context(|| format!("journal not found: {}", p.display()))?;
        let f = File::open(p).with_context(|| format!("open journal: {}", p.display()))?;
        let mut reader = BufReader::new(f);
        let mut buf = String::new();
        reader.read_to_string(&mut buf)?;
        self.lines = buf.lines().map(|s| s.to_string()).collect();
        self.enforce_capacity();
        self.last_size = meta.len();
        self.select_from_end(seek_last);
        self.ensure_selection_in_bounds();
        self.refresh_status();
        Ok(())
    }

    fn poll_updates(&mut self) {
        let Some(journal_path) = &self.journal_path else {
            return;
        };
        if !self.follow {
            return;
        }
        let Ok(meta) = fs::metadata(journal_path) else {
            return;
        };
        if meta.len() <= self.last_size {
            return;
        }
        if let Ok(mut f) = File::open(journal_path) {
            use std::io::Seek;
            use std::io::SeekFrom;
            if f.seek(SeekFrom::Start(self.last_size)).is_ok() {
                let reader = BufReader::new(f);
                for line in reader.lines().map_while(Result::ok) {
                    self.lines.push(line);
                }
                self.last_size = meta.len();
                let removed = self.enforce_capacity();
                if removed > 0 {
                    self.selected = self.selected.saturating_sub(removed);
                }
                if self.follow {
                    self.select_from_end(Some(0));
                } else {
                    self.ensure_selection_in_bounds();
                }
                self.refresh_status();
            }
        }
    }

    fn select_from_end(&mut self, seek_last: Option<usize>) {
        if self.lines.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.lines.len();
        let offset = seek_last.unwrap_or(0).min(len.saturating_sub(1));
        self.selected = len - 1 - offset;
    }

    fn ensure_selection_in_bounds(&mut self) {
        if self.lines.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.lines.len() {
            self.selected = self.lines.len() - 1;
        }
    }

    fn enforce_capacity(&mut self) -> usize {
        let mut removed = 0usize;
        while self.lines.len() > self.max_events {
            self.lines.remove(0);
            removed += 1;
        }
        removed
    }

    fn refresh_status(&mut self) {
        if self.diff.is_some() {
            if let Some(diff) = self.diff.as_ref() {
                self.status = diff.status_line();
            }
            return;
        }
        if self.recipes.visible {
            let recipes_status = self.recipes.status_line();
            if let Some(info) = recipes_status {
                self.status = format!("{} | {}", self.base_status, info);
            } else {
                self.status = format!(
                    "{} | Recipes ({} entries)",
                    self.base_status,
                    self.recipes.entries.len()
                );
            }
            return;
        }
        if self.lines.is_empty() {
            self.status = self.base_status.clone();
        } else {
            let position = self.selected.min(self.lines.len() - 1) + 1;
            self.status = format!(
                "{} | Event {}/{}",
                self.base_status,
                position,
                self.lines.len()
            );
        }
    }

    fn selected_line(&self) -> Option<&str> {
        self.lines.get(self.selected).map(String::as_str)
    }

    fn event_detail_text(&self) -> String {
        let Some(line) = self.selected_line() else {
            return "No events loaded".into();
        };
        let pretty = match serde_json::from_str::<serde_json::Value>(line) {
            Ok(json) => serde_json::to_string_pretty(&json).unwrap_or_else(|_| line.to_string()),
            Err(_) => line.to_string(),
        };
        if pretty.len() > MAX_EVENT_DETAIL_BYTES {
            let mut truncated = pretty[..MAX_EVENT_DETAIL_BYTES].to_string();
            truncated.push_str("\n... (truncated)");
            truncated
        } else {
            pretty
        }
    }

    fn selected_display_index(&self) -> Option<usize> {
        if self.lines.is_empty() {
            None
        } else {
            let idx = self.selected.min(self.lines.len() - 1);
            Some(self.lines.len() - 1 - idx)
        }
    }

    fn headless_output(&self) -> String {
        if self.lines.is_empty() {
            "No events loaded".to_string()
        } else {
            self.event_detail_text()
        }
    }

    fn toggle_recipes(&mut self) {
        if self.recipes.visible {
            self.recipes.visible = false;
            self.recipes.info = None;
            self.recipes.error = None;
            self.refresh_status();
            return;
        }
        match fetch_recipe_entries() {
            Ok(entries) => {
                self.recipes.visible = true;
                self.recipes.entries = entries;
                self.recipes.selected = 0;
                self.recipes.output.clear();
                self.recipes.error = None;
                self.recipes.diff_path = None;
                self.recipes.mode = RecipeMode::Idle;
                if self.recipes.entries.is_empty() {
                    self.recipes.info = Some("No recipes available".to_string());
                } else {
                    self.recipes.info = Some(
                        "Select a recipe and press Enter for dry-run (O diff, A apply)".to_string(),
                    );
                }
                self.refresh_status();
            }
            Err(err) => {
                self.recipes.visible = true;
                self.recipes.entries.clear();
                self.recipes.output = vec![format!("devit recipe list failed: {}", err)];
                self.recipes.error = Some("Recipe list failed".to_string());
                self.recipes.info = None;
                self.recipes.diff_path = None;
                self.recipes.mode = RecipeMode::Idle;
                self.refresh_status();
            }
        }
    }

    fn handle_recipe_key(&mut self, key: KeyCode) -> bool {
        if !self.recipes.visible {
            return false;
        }
        match key {
            KeyCode::Esc | KeyCode::Char('r') | KeyCode::Char('R') => {
                self.recipes.visible = false;
                self.refresh_status();
                true
            }
            KeyCode::Up => {
                if self.recipes.selected > 0 {
                    self.recipes.selected -= 1;
                }
                if let Some(entry) = self.recipes.selected_entry() {
                    self.recipes.info = Some(format!("Selected {}", entry.name));
                }
                self.refresh_status();
                true
            }
            KeyCode::Down => {
                if !self.recipes.entries.is_empty() {
                    let max = self.recipes.entries.len() - 1;
                    self.recipes.selected = (self.recipes.selected + 1).min(max);
                }
                if let Some(entry) = self.recipes.selected_entry() {
                    self.recipes.info = Some(format!("Selected {}", entry.name));
                }
                self.refresh_status();
                true
            }
            KeyCode::Enter => {
                self.run_recipe_dry_run();
                true
            }
            KeyCode::Char('o') | KeyCode::Char('O') => {
                self.open_recipe_diff();
                true
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.apply_recipe();
                true
            }
            _ => false,
        }
    }

    fn run_recipe_dry_run(&mut self) {
        let Some(entry) = self.recipes.selected_entry().cloned() else {
            self.recipes.info = Some("No recipe selected".to_string());
            self.refresh_status();
            return;
        };
        let id = entry.id.clone();
        let args = ["recipe", "run", id.as_str(), "--dry-run"];
        match run_devit_command(args) {
            Ok(output) => {
                self.recipes.output = collect_output_lines(&output);
                self.recipes.error = None;
                self.recipes.diff_path = detect_diff_path(&output);
                if output.success() {
                    self.recipes.mode = RecipeMode::DryRunReady { id: id.clone() };
                    if self.recipes.diff_path.is_some() {
                        self.recipes.info = Some(format!(
                            "Dry-run ready for {} (O diff, A apply)",
                            entry.name
                        ));
                    } else {
                        self.recipes.info = Some(format!(
                            "Dry-run ready for {} (no diff reported)",
                            entry.name
                        ));
                    }
                } else {
                    self.recipes.mode = RecipeMode::Idle;
                    if let Some(info) = detect_approval_required(&output) {
                        self.recipes.error = Some("Dry-run requires approval".to_string());
                        let _ = append_approval_notice(&entry.id, &info);
                    } else {
                        self.recipes.error = Some(format!(
                            "Dry-run failed (exit {})",
                            output.status.code().unwrap_or(-1)
                        ));
                    }
                }
            }
            Err(err) => {
                self.recipes.output = vec![format!("Failed to run devit: {}", err)];
                self.recipes.error = Some("Dry-run failed".to_string());
                self.recipes.mode = RecipeMode::Idle;
            }
        }
        self.refresh_status();
    }

    fn open_recipe_diff(&mut self) {
        let id = match &self.recipes.mode {
            RecipeMode::DryRunReady { id } => id.clone(),
            RecipeMode::Applying { id } => id.clone(),
            RecipeMode::Idle => {
                self.recipes.info = Some("Run dry-run before opening diff".to_string());
                self.refresh_status();
                return;
            }
        };
        let Some(path) = self.recipes.diff_path.clone() else {
            self.recipes.info = Some("Dry-run did not provide a diff".to_string());
            self.refresh_status();
            return;
        };
        match load_diff(&path, DiffSource::Path, 1_048_576) {
            Ok(diff_state) => {
                let status_line = diff_state.status_line();
                self.status = status_line;
                self.diff = Some(diff_state);
                self.recipes.visible = false;
                self.recipes.info = Some(format!("Diff opened for recipe {}", id));
            }
            Err(err) => {
                let msg = match err {
                    DiffError::NotFound => "diff file not found".to_string(),
                    DiffError::TooLarge => "diff too large".to_string(),
                    DiffError::Parse(e) => format!("diff parse error: {}", e),
                };
                self.recipes.error = Some(msg);
            }
        }
        self.refresh_status();
    }

    fn apply_recipe(&mut self) {
        let id = match &self.recipes.mode {
            RecipeMode::DryRunReady { id } => id.clone(),
            RecipeMode::Applying { id } => id.clone(),
            RecipeMode::Idle => {
                self.recipes.info = Some("Run dry-run before applying".to_string());
                self.refresh_status();
                return;
            }
        };
        self.recipes.mode = RecipeMode::Applying { id: id.clone() };
        self.recipes.info = Some(format!("Applying recipe {}...", id));
        self.refresh_status();
        let args = ["recipe", "run", id.as_str()];
        match run_devit_command(args) {
            Ok(output) => {
                self.recipes.output = collect_output_lines(&output);
                if output.success() {
                    self.recipes.mode = RecipeMode::Idle;
                    self.recipes.error = None;
                    self.recipes.info = Some(format!("Recipe '{}' applied", id));
                    self.recipes.diff_path = None;
                    self.diff = None;
                } else if let Some(info) = detect_approval_required(&output) {
                    self.recipes.mode = RecipeMode::DryRunReady { id: id.clone() };
                    self.recipes.error = Some("Approval required before applying".to_string());
                    let _ = append_approval_notice(&id, &info);
                } else {
                    self.recipes.mode = RecipeMode::DryRunReady { id: id.clone() };
                    self.recipes.error = Some(format!(
                        "Recipe apply failed (exit {})",
                        output.status.code().unwrap_or(-1)
                    ));
                }
            }
            Err(err) => {
                self.recipes.error = Some(format!("Failed to run devit: {}", err));
                self.recipes.mode = RecipeMode::DryRunReady { id };
            }
        }
        self.refresh_status();
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RecipeEntry {
    id: String,
    name: String,
    description: Option<String>,
}

#[derive(Debug, Default)]
struct RecipeState {
    visible: bool,
    entries: Vec<RecipeEntry>,
    selected: usize,
    output: Vec<String>,
    info: Option<String>,
    error: Option<String>,
    diff_path: Option<PathBuf>,
    mode: RecipeMode,
}

#[derive(Debug, Clone, Default)]
enum RecipeMode {
    #[default]
    Idle,
    DryRunReady {
        id: String,
    },
    Applying {
        id: String,
    },
}

impl RecipeState {
    fn status_line(&self) -> Option<String> {
        if let Some(err) = &self.error {
            return Some(format!("Recipe error: {}", err));
        }
        if let Some(info) = &self.info {
            return Some(info.clone());
        }
        match &self.mode {
            RecipeMode::Idle => None,
            RecipeMode::DryRunReady { id } => Some(format!(
                "Recipe '{}' ready: Enter=run again, O=open diff, A=apply",
                id
            )),
            RecipeMode::Applying { id } => Some(format!("Applying recipe '{}'", id)),
        }
    }

    fn selected_entry(&self) -> Option<&RecipeEntry> {
        self.entries.get(self.selected)
    }
}

#[derive(Debug)]
struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

impl CommandOutput {
    fn success(&self) -> bool {
        self.status.success()
    }
}

#[derive(Debug)]
struct ApprovalInfo {
    tool: Option<String>,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct RecipeListResponse {
    recipes: Vec<RecipeEntry>,
}

fn resolve_devit_bin() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for var in ["DEVIT_TUI_DEVIT_BIN", "DEVIT_BIN", "DEVIT_CLI_BIN"] {
        if let Ok(value) = env::var(var) {
            if !value.trim().is_empty() {
                candidates.push(PathBuf::from(value));
            }
        }
    }

    if let Ok(current) = env::current_exe() {
        if let Some(parent) = current.parent() {
            candidates.push(parent.join("devit"));
            candidates
                .push(parent.join(format!("devit{}", if cfg!(windows) { ".exe" } else { "" })));
            if let Some(root) = parent.parent() {
                candidates.push(root.join("release").join("devit"));
                candidates.push(
                    root.join("release")
                        .join(format!("devit{}", if cfg!(windows) { ".exe" } else { "" })),
                );
            }
        }
    }

    candidates.push(PathBuf::from(if cfg!(windows) {
        "devit.exe"
    } else {
        "devit"
    }));

    for candidate in &candidates {
        if (candidate.is_absolute() || candidate.components().count() > 1) && candidate.exists() {
            return candidate.clone();
        }
    }

    candidates
        .into_iter()
        .last()
        .unwrap_or_else(|| PathBuf::from("devit"))
}

fn run_devit_command<I, S>(args: I) -> Result<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let bin = resolve_devit_bin();
    let output = Command::new(&bin)
        .args(args)
        .output()
        .with_context(|| format!("spawn devit command {:?}", bin))?;
    Ok(CommandOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn fetch_recipe_entries() -> Result<Vec<RecipeEntry>> {
    let output = run_devit_command(["recipe", "list"])?;
    if !output.success() {
        bail!("devit recipe list failed: {}", output.stderr.trim());
    }
    let parsed: RecipeListResponse =
        serde_json::from_str(output.stdout.trim()).context("parse devit recipe list output")?;
    Ok(parsed.recipes)
}

fn list_recipes_headless() -> Result<()> {
    let output = run_devit_command(["recipe", "list"])?;
    if !output.success() {
        if !output.stderr.trim().is_empty() {
            eprintln!("{}", output.stderr.trim());
        }
        // Normalized error for scripting
        let _ = writeln!(
            std::io::stderr(),
            "{}",
            serde_json::json!({
                "type":"tool.error",
                "error":{ "recipe_integration_failed": true, "reason":"list_failed", "status": output.status.code().unwrap_or(-1) }
            })
        );
        bail!("devit recipe list failed");
    }
    print!("{}", output.stdout);
    Ok(())
}

fn run_recipe_headless(id: &str, dry_run: bool) -> Result<i32> {
    let mut args = vec!["recipe", "run", id];
    if dry_run {
        args.push("--dry-run");
    }
    let output = run_devit_command(args)?;

    if let Some(info) = detect_approval_required(&output) {
        // Surface approval_required as-is by passing through stdout/stderr, plus normalized code 2
        if !output.stdout.trim().is_empty() {
            print!("{}", output.stdout);
        }
        if !output.stderr.trim().is_empty() {
            eprintln!("{}", output.stderr);
        }
        let _ = writeln!(
            std::io::stderr(),
            "{}",
            serde_json::json!({
                "type":"tui.notice",
                "payload":{ "approval_required": true, "recipe": id, "tool": info.tool, "reason": info.reason }
            })
        );
        return Ok(2);
    }

    if !output.success() {
        if !output.stdout.trim().is_empty() {
            print!("{}", output.stdout);
        }
        if !output.stderr.trim().is_empty() {
            eprintln!("{}", output.stderr);
        }
        let _ = writeln!(
            std::io::stderr(),
            "{}",
            serde_json::json!({
                "type":"tool.error",
                "error":{ "recipe_integration_failed": true, "reason":"run_failed", "status": output.status.code().unwrap_or(-1) }
            })
        );
        return Ok(output.status.code().unwrap_or(1));
    }

    // Success: print through devit CLI output
    if !output.stdout.trim().is_empty() {
        print!("{}", output.stdout);
    }
    // If dry-run but no diff detected, emit a normalized note (non-fatal)
    if dry_run && detect_diff_path(&output).is_none() {
        let _ = writeln!(
            std::io::stderr(),
            "{}",
            serde_json::json!({
                "type":"tool.error",
                "error":{ "recipe_integration_failed": true, "reason":"no_patch" }
            })
        );
    }
    Ok(0)
}

fn collect_output_lines(output: &CommandOutput) -> Vec<String> {
    let mut lines = Vec::new();
    if !output.stdout.trim().is_empty() {
        lines.extend(output.stdout.lines().map(|s| s.to_string()));
    }
    if !output.stderr.trim().is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("stderr:".to_string());
        lines.extend(output.stderr.lines().map(|s| s.to_string()));
    }
    if lines.is_empty() {
        lines.push("(no output)".to_string());
    }
    lines
}

fn detect_diff_path(output: &CommandOutput) -> Option<PathBuf> {
    for line in output.stdout.lines().chain(output.stderr.lines()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(path) = extract_diff_path(&value) {
                return Some(path);
            }
        }
    }
    None
}

fn extract_diff_path(value: &serde_json::Value) -> Option<PathBuf> {
    let mut candidates: Vec<Option<&str>> = Vec::new();
    candidates.push(value.get("diff_path").and_then(|v| v.as_str()));
    candidates.push(value.get("patch_path").and_then(|v| v.as_str()));
    if let Some(payload) = value.get("payload") {
        candidates.push(payload.get("diff_path").and_then(|v| v.as_str()));
        candidates.push(payload.get("patch_path").and_then(|v| v.as_str()));
    }
    if let Some(recipe) = value.get("recipe") {
        candidates.push(recipe.get("diff_path").and_then(|v| v.as_str()));
        candidates.push(recipe.get("patch_path").and_then(|v| v.as_str()));
    }
    for candidate in candidates.into_iter().flatten() {
        if !candidate.trim().is_empty() {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

fn detect_approval_required(output: &CommandOutput) -> Option<ApprovalInfo> {
    for line in output.stdout.lines().chain(output.stderr.lines()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(info) = approval_info_from_value(&value) {
                return Some(info);
            }
        }
    }
    None
}

fn approval_info_from_value(value: &serde_json::Value) -> Option<ApprovalInfo> {
    if value
        .get("approval_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some(ApprovalInfo {
            tool: value
                .get("tool")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            reason: value
                .get("reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });
    }
    if let Some(payload) = value.get("payload") {
        if payload
            .get("approval_required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Some(ApprovalInfo {
                tool: payload
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                reason: payload
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            });
        }
    }
    None
}

fn append_approval_notice(recipe_id: &str, info: &ApprovalInfo) -> Result<()> {
    let payload = serde_json::json!({
        "type": "tui.notice",
        "payload": {
            "approval_required": true,
            "recipe": recipe_id,
            "tool": info.tool,
            "reason": info.reason,
        }
    });
    let path = Path::new(".devit").join("journal.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .context("open journal for approval notice")?;
    writeln!(file, "{}", payload).context("write approval notice to journal")?;
    Ok(())
}

fn print_tool_error_journal_not_found(path: &PathBuf) {
    // Stable JSON error on stderr
    let _ = writeln!(
        std::io::stderr(),
        "{}",
        serde_json::json!({
            "type":"tool.error",
            "error":{
                "tui_io_error": true,
                "reason":"journal_not_found",
                "path": path,
            }
        })
    );
}

fn print_diff_error(reason: &str, path: &PathBuf) {
    let _ = writeln!(
        std::io::stderr(),
        "{}",
        serde_json::json!({
            "type":"tool.error",
            "error":{
                "diff_load_failed": true,
                "reason": reason,
                "path": path,
            }
        })
    );
}

fn print_diff_error_with_message(reason: &str, path: &PathBuf, message: &str) {
    let _ = writeln!(
        std::io::stderr(),
        "{}",
        serde_json::json!({
            "type":"tool.error",
            "error":{
                "diff_load_failed": true,
                "reason": reason,
                "path": path,
                "message": message,
            }
        })
    );
}

fn print_diff_error_stdin(reason: &str, message: &str) {
    let path = PathBuf::from("-");
    print_diff_error_with_message(reason, &path, message);
}

fn best_effort_status() -> String {
    // Try to query versions/policy; fall back silently on errors
    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let out = std::process::Command::new(cmd).args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    let ver_devit = run("devit", &["--version"]).unwrap_or_else(|| "devit N/A".into());
    let ver_mcpd = run("devit-mcpd", &["--version"]).unwrap_or_else(|| "mcpd N/A".into());
    // policy is optional and not parsed deeply here
    let policy = run("devit-mcp", &["--policy"]).unwrap_or_else(|| "policy N/A".into());
    format!("{} | {} | {}", ver_devit, ver_mcpd, policy)
}

fn main() -> Result<()> {
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> Result<()> {
    if args.list_recipes {
        list_recipes_headless()?;
        return Ok(());
    }

    if let Some(id) = args.run_recipe.as_deref() {
        let headless = headless_mode();
        if headless {
            let code = run_recipe_headless(id, args.dry_run)?;
            std::process::exit(code);
        }
        // Interactive path: execute dry-run, capture diff, open viewer if present, allow Apply
        let mut tui_app = App::new(None, false, best_effort_status(), DEFAULT_MAX_EVENTS);
        // Simulate the list toggle view state
        tui_app.recipes.visible = true;
        // Ensure entries include the target so status line can show meaningful info
        tui_app.recipes.entries = vec![RecipeEntry {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
        }];
        tui_app.recipes.selected = 0;
        let mut cmd = vec!["recipe", "run", id];
        if args.dry_run {
            cmd.push("--dry-run");
        }
        match run_devit_command(cmd) {
            Ok(output) => {
                tui_app.recipes.output = collect_output_lines(&output);
                tui_app.recipes.diff_path = detect_diff_path(&output);
                if output.success() {
                    tui_app.recipes.mode = RecipeMode::DryRunReady { id: id.to_string() };
                    if let Some(path) = tui_app.recipes.diff_path.clone() {
                        match load_diff(&path, DiffSource::Path, 1_048_576) {
                            Ok(diff_state) => {
                                tui_app.status = diff_state.status_line();
                                tui_app.base_status = tui_app.status.clone();
                                tui_app.diff = Some(diff_state);
                                tui_app.recipes.visible = false;
                                tui_app.recipes.info =
                                    Some(format!("Diff opened for recipe {}", id));
                            }
                            Err(err) => {
                                let msg = match err {
                                    DiffError::NotFound => "diff file not found".to_string(),
                                    DiffError::TooLarge => "diff too large".to_string(),
                                    DiffError::Parse(e) => format!("diff parse error: {}", e),
                                };
                                tui_app.recipes.error = Some(msg);
                            }
                        }
                    } else if args.dry_run {
                        tui_app.recipes.info =
                            Some("Dry-run succeeded (no patch to preview)".to_string());
                    }
                } else if let Some(_info) = detect_approval_required(&output) {
                    tui_app.recipes.mode = RecipeMode::DryRunReady { id: id.to_string() };
                    tui_app.recipes.error = Some("Approval required before running".to_string());
                } else {
                    tui_app.recipes.mode = RecipeMode::Idle;
                    tui_app.recipes.error = Some("Recipe run failed".to_string());
                }
            }
            Err(err) => {
                tui_app.recipes.error = Some(format!("Failed to run devit: {}", err));
                tui_app.recipes.mode = RecipeMode::Idle;
            }
        }
        tui_app.refresh_status();

        // Enter TUI once to show either diff or recipes view with output
        let guard = TerminalGuard::enter()?;
        let backend = CrosstermBackend::new(std::io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.hide_cursor()?;
        let mut control = LoopControl::interactive(false)?;
        let result = run_app(&mut terminal, &mut tui_app, &mut control);
        terminal.show_cursor().ok();
        drop(guard);
        return result;
    }

    let journal_path = args.open_log.clone().or_else(|| args.journal_path.clone());

    if journal_path.is_none() && args.open_target.is_none() {
        bail!("either --journal-path/--open-log or --open-diff must be provided");
    }

    if let Some(path) = &journal_path {
        if !path.exists() {
            print_tool_error_journal_not_found(path);
            bail!("journal missing");
        }
    }

    let headless = headless_mode();
    let initial_follow = if headless { false } else { args.follow };

    let max_events = args.seek_last.unwrap_or(DEFAULT_MAX_EVENTS).max(1);

    let base_status = best_effort_status();
    let mut app = App::new(
        journal_path.clone(),
        initial_follow,
        base_status,
        max_events,
    );
    app.load_initial(Some(0))?;

    if let Some(open_diff) = args.open_target.as_ref() {
        let source = if open_diff.as_os_str() == "-" {
            DiffSource::Stdin
        } else {
            DiffSource::Path
        };
        match load_diff(open_diff, source, 1_048_576) {
            Ok(diff_state) => {
                app.status = diff_state.status_line();
                app.base_status = app.status.clone();
                app.diff = Some(diff_state);
                app.follow = false;
            }
            Err(DiffError::NotFound) => {
                print_diff_error("not_found", open_diff);
                std::process::exit(2);
            }
            Err(DiffError::TooLarge) => {
                print_diff_error("too_large", open_diff);
                std::process::exit(2);
            }
            Err(DiffError::Parse(msg)) => {
                if open_diff.as_os_str() == "-" {
                    print_diff_error_stdin("parse_error", &msg);
                } else {
                    print_diff_error_with_message("parse_error", open_diff, &msg);
                }
                std::process::exit(2);
            }
        }
    }

    if journal_path.is_some() && args.open_target.is_none() && args.open_log.is_some() {
        app.follow = false;
    }

    if headless {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut control = LoopControl::headless();
        let result = run_app(&mut terminal, &mut app, &mut control);
        if result.is_ok() {
            println!("{}", app.headless_output());
        }
        return result;
    }

    let mut control = LoopControl::interactive(initial_follow)?;
    let guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    let result = run_app(&mut terminal, &mut app, &mut control);
    terminal.show_cursor().ok();
    drop(guard);
    result
}

fn headless_mode() -> bool {
    std::env::var("DEVIT_TUI_HEADLESS")
        .ok()
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return true;
            }
            matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(std::io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        disable_raw_mode().ok();
        let mut stdout = std::io::stdout();
        execute!(stdout, LeaveAlternateScreen, Show).ok();
    }
}

struct LoopControl {
    headless: bool,
    allow_block_without_follow: bool,
    follow_stop: Option<FollowStop>,
}

impl LoopControl {
    fn headless() -> Self {
        Self {
            headless: true,
            allow_block_without_follow: false,
            follow_stop: None,
        }
    }

    fn interactive(initial_follow: bool) -> Result<Self> {
        let follow_stop = if initial_follow {
            Some(FollowStop::install_ctrlc_handler()?)
        } else {
            None
        };
        Ok(Self {
            headless: false,
            allow_block_without_follow: true,
            follow_stop,
        })
    }

    fn ensure_follow_stop(&mut self) -> Result<()> {
        if self.follow_stop.is_none() {
            self.follow_stop = Some(FollowStop::install_ctrlc_handler()?);
        }
        Ok(())
    }
}

struct FollowStop {
    rx: Receiver<()>,
}

impl FollowStop {
    fn install_ctrlc_handler() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        ctrlc::set_handler(move || {
            let _ = tx.send(());
        })
        .context("install ctrl+c handler for follow mode")?;
        Ok(Self { rx })
    }

    fn should_stop(&mut self) -> bool {
        match self.rx.try_recv() {
            Ok(_) | Err(TryRecvError::Disconnected) => true,
            Err(TryRecvError::Empty) => false,
        }
    }
}

#[derive(Debug)]
enum DiffSource {
    Path,
    Stdin,
}

#[derive(Debug)]
enum DiffError {
    NotFound,
    TooLarge,
    Parse(String),
}

#[derive(Debug, Clone)]
struct DiffState {
    files: Vec<DiffFile>,
    file_idx: usize,
    hunk_idx: usize,
}

impl DiffState {
    fn new(files: Vec<DiffFile>) -> Self {
        Self {
            files,
            file_idx: 0,
            hunk_idx: 0,
        }
    }

    fn status_line(&self) -> String {
        if self.files.is_empty() {
            return "Diff: empty".to_string();
        }
        let file = &self.files[self.file_idx];
        let file_total = self.files.len();
        if file.hunks.is_empty() {
            format!(
                "Diff {}/{}: {} — no hunks",
                self.file_idx + 1,
                file_total,
                file.display_name
            )
        } else {
            format!(
                "Diff {}/{}: {} — hunk {}/{} (j/k hunks, h/H files)",
                self.file_idx + 1,
                file_total,
                file.display_name,
                self.hunk_idx + 1,
                file.hunks.len()
            )
        }
    }

    fn current(&self) -> Option<(&DiffFile, Option<&DiffHunk>)> {
        let file = self.files.get(self.file_idx)?;
        let hunk = file.hunks.get(self.hunk_idx);
        Some((file, hunk))
    }

    fn next_hunk(&mut self) -> bool {
        if self.files.is_empty() {
            return false;
        }
        let file = &self.files[self.file_idx];
        if file.hunks.is_empty() {
            return false;
        }
        if self.hunk_idx + 1 < file.hunks.len() {
            self.hunk_idx += 1;
            true
        } else {
            false
        }
    }

    fn prev_hunk(&mut self) -> bool {
        if self.files.is_empty() {
            return false;
        }
        if self.hunk_idx > 0 {
            self.hunk_idx -= 1;
            true
        } else {
            false
        }
    }

    fn next_file(&mut self) -> bool {
        if self.file_idx + 1 < self.files.len() {
            self.file_idx += 1;
            self.hunk_idx = 0;
            true
        } else {
            false
        }
    }

    fn prev_file(&mut self) -> bool {
        if self.file_idx > 0 {
            self.file_idx -= 1;
            self.hunk_idx = 0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
struct DiffFile {
    display_name: String,
    header: Vec<String>,
    hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone)]
struct DiffHunk {
    header: String,
    lines: Vec<String>,
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    control: &mut LoopControl,
) -> Result<()> {
    draw_frame(terminal, app)?;

    if control.headless {
        return Ok(());
    }

    let tick_rate = Duration::from_millis(150);
    let mut last_tick = Instant::now();

    'main: loop {
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if matches!(key.code, KeyCode::Char('q')) {
                        break Ok(());
                    }

                    if app.recipes.visible && app.handle_recipe_key(key.code) {
                        continue 'main;
                    }

                    if let Some(diff) = app.diff.as_mut() {
                        let mut updated = false;
                        match key.code {
                            KeyCode::Char('j') | KeyCode::Char('J') | KeyCode::Down => {
                                if diff.next_hunk() {
                                    updated = true;
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Char('K') | KeyCode::Up => {
                                if diff.prev_hunk() {
                                    updated = true;
                                }
                            }
                            KeyCode::Char('h') => {
                                if diff.prev_file() {
                                    updated = true;
                                }
                            }
                            KeyCode::Char('H') => {
                                if diff.next_file() {
                                    updated = true;
                                }
                            }
                            KeyCode::Char('a') | KeyCode::Char('A') => {
                                if matches!(app.recipes.mode, RecipeMode::DryRunReady { .. }) {
                                    app.apply_recipe();
                                    if app.diff.is_some() {
                                        app.diff = None;
                                    }
                                }
                            }
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                app.diff = None;
                                app.toggle_recipes();
                            }
                            KeyCode::Esc => {
                                app.diff = None;
                                app.refresh_status();
                            }
                            KeyCode::F(1) => app.help = !app.help,
                            _ => {}
                        }
                        if let Some(diff) = app.diff.as_ref() {
                            if updated {
                                app.status = diff.status_line();
                            }
                        } else {
                            app.refresh_status();
                        }
                        continue 'main;
                    }

                    match key.code {
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            app.toggle_recipes();
                            continue 'main;
                        }
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            if matches!(app.recipes.mode, RecipeMode::DryRunReady { .. }) {
                                app.apply_recipe();
                            }
                        }
                        KeyCode::Char('f') => {
                            app.follow = !app.follow;
                            if app.follow {
                                control.ensure_follow_stop()?;
                                app.select_from_end(Some(0));
                            }
                            app.refresh_status();
                        }
                        KeyCode::Up => {
                            let prev = app.selected;
                            if app.selected > 0 {
                                app.selected -= 1;
                            }
                            if app.selected != prev {
                                app.refresh_status();
                            }
                        }
                        KeyCode::Down => {
                            let prev = app.selected;
                            if !app.lines.is_empty() {
                                let max = app.lines.len() - 1;
                                app.selected = (app.selected + 1).min(max);
                            }
                            if app.selected != prev {
                                app.refresh_status();
                            }
                        }
                        KeyCode::Char('/') => {
                            app.status = format!("search: not implemented | {}", app.status);
                        }
                        KeyCode::F(1) => app.help = !app.help,
                        _ => {}
                    }
                }
            }
        }

        if !control.allow_block_without_follow && !app.follow {
            return Ok(());
        }

        if let Some(stop) = control.follow_stop.as_mut() {
            if stop.should_stop() {
                return Ok(());
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.poll_updates();
            last_tick = Instant::now();
        }

        draw_frame(terminal, app)?;
    }
}

fn draw_frame<B: Backend>(terminal: &mut Terminal<B>, app: &App) -> Result<()> {
    terminal.draw(|f| {
        let size = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)].as_ref())
            .split(size);

        if let Some(diff) = &app.diff {
            draw_diff_view(f, chunks[0], diff);
        } else if app.recipes.visible {
            draw_recipe_view(f, chunks[0], &app.recipes);
        } else {
            let title = Span::raw("Timeline");
            let block = Block::default().title(title).borders(Borders::ALL);
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(chunks[0]);

            let items: Vec<ListItem> = app
                .lines
                .iter()
                .rev()
                .map(|l| ListItem::new(Line::from(l.as_str())))
                .collect();
            let list = List::new(items)
                .block(block)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            let mut state = ListState::default();
            if let Some(idx) = app.selected_display_index() {
                state.select(Some(idx));
            }
            f.render_stateful_widget(list, main_chunks[0], &mut state);

            let detail_title = if app.lines.is_empty() {
                "Event (none)".to_string()
            } else {
                let pos = app.selected.min(app.lines.len() - 1) + 1;
                format!("Event {} of {}", pos, app.lines.len())
            };
            let detail = Paragraph::new(app.event_detail_text())
                .block(Block::default().title(detail_title).borders(Borders::ALL))
                .wrap(Wrap { trim: false });
            f.render_widget(detail, main_chunks[1]);
        }

        let status = Paragraph::new(Line::from(app.status.clone())).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::raw("Status")),
        );
        f.render_widget(status, chunks[1]);

        if app.help {
            let help_text = if app.recipes.visible {
                "Recipes: Enter=dry-run, O=open diff, A=apply, R/Esc=close"
            } else if app.diff.is_some() {
                "Diff keys: q=quit, Esc=close, j/k hunk ±, h/H file ±, a=apply"
            } else {
                "Keys: q=quit, f=follow, r=recipes, ↑/↓ navigate, /=search, F1=help"
            };
            let area = centered_rect(60, 40, size);
            let help = Paragraph::new(help_text)
                .block(Block::default().title("Help").borders(Borders::ALL));
            f.render_widget(help, area);
        }
    })?;
    Ok(())
}

fn draw_recipe_view(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    state: &RecipeState,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
        .split(area);

    let items: Vec<ListItem> = if state.entries.is_empty() {
        vec![ListItem::new(Line::from("No recipes available"))]
    } else {
        state
            .entries
            .iter()
            .map(|entry| {
                let mut text = entry.name.clone();
                if let Some(desc) = &entry.description {
                    if !desc.trim().is_empty() {
                        text.push_str(" — ");
                        text.push_str(desc.trim());
                    }
                }
                ListItem::new(Line::from(text))
            })
            .collect()
    };

    let list = List::new(items)
        .block(Block::default().title("Recipes").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default();
    if !state.entries.is_empty() {
        list_state.select(Some(state.selected.min(state.entries.len() - 1)));
    }
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    let mut lines: Vec<Line> = Vec::new();
    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }
    if !state.output.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        for line in &state.output {
            lines.push(Line::from(line.clone()));
        }
    }
    if let Some(info) = &state.info {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            info.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(
            "Select a recipe and press Enter to run a dry-run (O diff, A apply).",
        ));
    }

    let details = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().title("Details").borders(Borders::ALL));
    frame.render_widget(details, chunks[1]);
}

fn draw_diff_view(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, diff: &DiffState) {
    let block_title = if let Some((file, _)) = diff.current() {
        format!(
            "Diff: {} ({}/{})",
            file.display_name,
            diff.file_idx + 1,
            diff.files.len()
        )
    } else {
        "Diff".to_string()
    };

    let mut lines: Vec<Line> = Vec::new();
    if let Some((file, hunk_opt)) = diff.current() {
        if !file.header.is_empty() {
            for header in &file.header {
                lines.push(Line::from(header.clone()));
            }
        }
        if let Some(hunk) = hunk_opt {
            lines.push(Line::from(hunk.header.clone()));
            for body_line in &hunk.lines {
                let style = if body_line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if body_line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(body_line.clone(), style)));
            }
        } else {
            lines.push(Line::from("(no hunks)"));
        }
    } else {
        lines.push(Line::from("No diff content"));
    }

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .title(Span::raw(block_title))
            .borders(Borders::ALL),
    );
    frame.render_widget(paragraph, area);
}

fn load_diff(path: &PathBuf, source: DiffSource, max_size: usize) -> Result<DiffState, DiffError> {
    let content = match source {
        DiffSource::Path => {
            let metadata = fs::metadata(path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    DiffError::NotFound
                } else {
                    DiffError::Parse(e.to_string())
                }
            })?;
            if metadata.len() as usize > max_size {
                return Err(DiffError::TooLarge);
            }
            let mut buf = String::new();
            File::open(path)
                .and_then(|mut f| f.read_to_string(&mut buf))
                .map_err(|e| DiffError::Parse(e.to_string()))?;
            buf
        }
        DiffSource::Stdin => {
            let mut buf = String::new();
            let mut handle = std::io::stdin().lock();
            handle
                .read_to_string(&mut buf)
                .map_err(|e| DiffError::Parse(e.to_string()))?;
            if buf.len() > max_size {
                return Err(DiffError::TooLarge);
            }
            buf
        }
    };

    let files = parse_unified_diff(&content).map_err(DiffError::Parse)?;
    if files.is_empty() {
        return Err(DiffError::Parse("empty diff".to_string()));
    }
    Ok(DiffState::new(files))
}

fn parse_unified_diff(content: &str) -> Result<Vec<DiffFile>, String> {
    #[derive(Default)]
    struct PartialFile {
        header: Vec<String>,
        hunks: Vec<DiffHunk>,
        old_path: Option<String>,
        new_path: Option<String>,
        diff_header: Option<String>,
    }

    impl PartialFile {
        fn with_diff_header(line: &str) -> Self {
            PartialFile {
                diff_header: Some(line.to_string()),
                header: vec![line.to_string()],
                ..Default::default()
            }
        }

        fn finalize(self) -> DiffFile {
            let display = self
                .new_path
                .as_ref()
                .or(self.old_path.as_ref())
                .cloned()
                .or_else(|| {
                    self.diff_header
                        .as_ref()
                        .and_then(|h| extract_from_diff_header(h))
                })
                .unwrap_or_else(|| "(unknown)".to_string());
            DiffFile {
                display_name: clean_diff_path(&display),
                header: self.header,
                hunks: self.hunks,
            }
        }
    }

    let mut files: Vec<DiffFile> = Vec::new();
    let mut current_file: Option<PartialFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;

    let flush_hunk = |file: &mut Option<PartialFile>, hunk: &mut Option<DiffHunk>| {
        if let Some(h) = hunk.take() {
            if file.is_none() {
                *file = Some(PartialFile::default());
            }
            if let Some(f) = file.as_mut() {
                f.hunks.push(h);
            }
        }
    };

    let flush_file =
        |files: &mut Vec<DiffFile>, file: &mut Option<PartialFile>, hunk: &mut Option<DiffHunk>| {
            flush_hunk(file, hunk);
            if let Some(pf) = file.take() {
                files.push(pf.finalize());
            }
        };

    for line in content.lines() {
        if line.starts_with("diff --git") {
            flush_file(&mut files, &mut current_file, &mut current_hunk);
            current_file = Some(PartialFile::with_diff_header(line));
            continue;
        }

        if line.starts_with("@@") {
            if current_file.is_none() {
                current_file = Some(PartialFile::default());
            }
            flush_hunk(&mut current_file, &mut current_hunk);
            current_hunk = Some(DiffHunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
            continue;
        }

        if let Some(hunk) = current_hunk.as_mut() {
            hunk.lines.push(line.to_string());
            continue;
        }

        if current_file.is_none() {
            current_file = Some(PartialFile::default());
        }

        if let Some(file) = current_file.as_mut() {
            if line.starts_with("--- ") {
                file.old_path = extract_path_after_prefix(line);
            }
            if line.starts_with("+++ ") {
                file.new_path = extract_path_after_prefix(line);
            }
            file.header.push(line.to_string());
        }
    }

    flush_file(&mut files, &mut current_file, &mut current_hunk);

    Ok(files)
}

fn extract_path_after_prefix(line: &str) -> Option<String> {
    line.split_whitespace().nth(1).map(clean_diff_path)
}

fn clean_diff_path(raw: &str) -> String {
    let trimmed = raw.trim_matches('"');
    let without_prefix = trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed);
    without_prefix.to_string()
}

fn extract_from_diff_header(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    // Expect format: diff --git a/path b/path
    let first = parts.find(|part| part.starts_with('a'))?;
    let second = parts.next();
    second.or(Some(first)).map(clean_diff_path)
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1]);
    horizontal[1]
}

// tests moved to integration tests
