//! Application state and lifecycle management.
//!
//! This module provides the core [`App`] struct which manages the entire
//! application state, including the scanner, file watcher, UI state,
//! and event handling.
//!
//! # Architecture
//!
//! ```text
//! App
//!  ├── scanner: Scanner          # File analysis results
//!  ├── watcher: FileWatcher      # Live file change detection
//!  ├── mode: AppMode             # Current UI mode
//!  ├── focus: Focus              # Active panel
//!  ├── file_list_state: FileListState
//!  ├── detail_state: DetailPaneState
//!  ├── filter: FilterState       # Current filter configuration
//!  └── status: Option<StatusMessage>
//! ```

use std::time::Instant;

use camino::Utf8PathBuf;
use ch_core::{Config, FileInfo, MigrationStatus};
use ch_scanner::{ScanConfig as ScannerConfig, ScanResult, Scanner, StatsSnapshot};
use ch_ts_parser::ModelPathMatcher;
use ch_watcher::FileEvent;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::layout::Rect;
use tracing::{debug, info, warn};

use crate::action::Action;
use crate::error::TuiError;

/// The current mode of the application UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    /// Normal browsing mode.
    #[default]
    Normal,

    /// Filter input mode (typing a filter).
    Filtering,

    /// Help panel is displayed.
    Help,

    /// Directory setup overlay is displayed.
    DirectorySetup,
}

/// Which panel has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// File list panel is focused.
    #[default]
    FileList,

    /// Detail pane is focused.
    DetailPane,
}

impl Focus {
    /// Toggles between `FileList` and `DetailPane`.
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::FileList => Self::DetailPane,
            Self::DetailPane => Self::FileList,
        }
    }
}

/// State for the file list widget.
#[derive(Debug, Clone, Default)]
pub struct FileListState {
    /// Currently selected index (if any).
    pub selected: Option<usize>,

    /// Scroll offset for virtualized rendering.
    pub scroll_offset: usize,

    /// Indices of files after filtering.
    /// If `None`, all files are shown.
    filtered_indices: Option<Vec<usize>>,

    /// Height of the visible area (for page navigation).
    pub visible_height: usize,
}

impl FileListState {
    /// Creates a new file list state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of items in the filtered list.
    #[must_use]
    pub fn len(&self, total_files: usize) -> usize {
        self.filtered_indices
            .as_ref()
            .map_or(total_files, Vec::len)
    }

    /// Returns `true` if the filtered list is empty.
    #[must_use]
    pub fn is_empty(&self, total_files: usize) -> bool {
        self.len(total_files) == 0
    }

    /// Moves selection to the next item.
    pub fn select_next(&mut self, total_files: usize) {
        let len = self.len(total_files);
        if len == 0 {
            self.selected = None;
            return;
        }

        self.selected = Some(match self.selected {
            Some(i) if i + 1 < len => i + 1,
            Some(_) | None => 0, // Wrap to start
        });

        self.ensure_visible();
    }

    /// Moves selection to the previous item.
    pub fn select_previous(&mut self, total_files: usize) {
        let len = self.len(total_files);
        if len == 0 {
            self.selected = None;
            return;
        }

        self.selected = Some(match self.selected {
            Some(0) | None => len.saturating_sub(1), // Wrap to end
            Some(i) => i - 1,
        });

        self.ensure_visible();
    }

    /// Moves selection to the first item.
    pub fn select_first(&mut self, total_files: usize) {
        let len = self.len(total_files);
        if len == 0 {
            self.selected = None;
        } else {
            self.selected = Some(0);
            self.scroll_offset = 0;
        }
    }

    /// Moves selection to the last item.
    pub fn select_last(&mut self, total_files: usize) {
        let len = self.len(total_files);
        if len == 0 {
            self.selected = None;
        } else {
            self.selected = Some(len - 1);
            self.ensure_visible();
        }
    }

    /// Moves selection down by one page.
    pub fn page_down(&mut self, total_files: usize) {
        let len = self.len(total_files);
        if len == 0 {
            return;
        }

        let page_size = self.visible_height.max(1);
        self.selected = Some(match self.selected {
            Some(i) => (i + page_size).min(len - 1),
            None => page_size.min(len - 1),
        });

        self.ensure_visible();
    }

    /// Moves selection up by one page.
    pub fn page_up(&mut self, total_files: usize) {
        let len = self.len(total_files);
        if len == 0 {
            return;
        }

        let page_size = self.visible_height.max(1);
        self.selected = Some(match self.selected {
            Some(i) => i.saturating_sub(page_size),
            None => 0,
        });

        self.ensure_visible();
    }

    /// Selects a specific item by index.
    pub fn select(&mut self, index: usize, total_files: usize) {
        let len = self.len(total_files);
        if index < len {
            self.selected = Some(index);
            self.ensure_visible();
        }
    }

    /// Sets the filtered indices.
    pub fn set_filter(&mut self, indices: Option<Vec<usize>>) {
        self.filtered_indices = indices;
        // Reset selection when filter changes
        self.selected = if self.filtered_indices.as_ref().is_some_and(|v| !v.is_empty()) {
            Some(0)
        } else {
            None
        };
        self.scroll_offset = 0;
    }

    /// Clears the filter.
    pub fn clear_filter(&mut self) {
        self.filtered_indices = None;
    }

    /// Returns the actual file index for a display index.
    #[must_use]
    pub fn actual_index(&self, display_index: usize) -> usize {
        self.filtered_indices
            .as_ref()
            .and_then(|indices| indices.get(display_index).copied())
            .unwrap_or(display_index)
    }

    /// Returns the filtered indices (or `None` if no filter).
    #[must_use]
    pub fn filtered_indices(&self) -> Option<&[usize]> {
        self.filtered_indices.as_deref()
    }

    /// Ensures the selected item is visible.
    fn ensure_visible(&mut self) {
        if let Some(selected) = self.selected {
            if selected < self.scroll_offset {
                self.scroll_offset = selected;
            } else if selected >= self.scroll_offset + self.visible_height {
                self.scroll_offset = selected.saturating_sub(self.visible_height - 1);
            }
        }
    }
}

/// State for the detail pane widget.
#[derive(Debug, Clone, Default)]
pub struct DetailPaneState {
    /// Scroll offset within the detail view.
    pub scroll_offset: usize,
}

/// Filter configuration state.
#[derive(Debug, Clone, Default)]
pub struct FilterState {
    /// Text filter for file paths.
    pub text: String,

    /// Status filter (show only files with this status).
    pub status: Option<MigrationStatus>,
}

/// Field focus for directory setup input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryField {
    /// Root path (WebApp.Desktop/src).
    Root,
    /// Legacy shared directory path.
    Shared,
    /// New `shared_2023` directory path.
    Shared2023,
}

impl DirectoryField {
    /// Returns the next field in focus order.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Root => Self::Shared,
            Self::Shared => Self::Shared2023,
            Self::Shared2023 => Self::Root,
        }
    }

    /// Returns the previous field in focus order.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Root => Self::Shared2023,
            Self::Shared => Self::Root,
            Self::Shared2023 => Self::Shared,
        }
    }
}

/// Directory setup input state.
#[derive(Debug, Clone)]
pub struct DirectorySetup {
    /// Input value for root path.
    pub root_input: String,
    /// Input value for shared path.
    pub shared_input: String,
    /// Input value for `shared_2023` path.
    pub shared_2023_input: String,
    /// Which field is active.
    pub active_field: DirectoryField,
}

impl DirectorySetup {
    /// Creates directory input state from the current configuration.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        Self {
            root_input: config.scan.root_path.to_string(),
            shared_input: config.scan.shared_path.to_string(),
            shared_2023_input: config.scan.shared_2023_path.to_string(),
            active_field: DirectoryField::Root,
        }
    }

    /// Refreshes input values from the current configuration.
    pub fn refresh_from_config(&mut self, config: &Config) {
        self.root_input = config.scan.root_path.to_string();
        self.shared_input = config.scan.shared_path.to_string();
        self.shared_2023_input = config.scan.shared_2023_path.to_string();
        self.active_field = DirectoryField::Root;
    }

    /// Moves focus to the next input field.
    pub fn focus_next(&mut self) {
        self.active_field = self.active_field.next();
    }

    /// Moves focus to the previous input field.
    pub fn focus_previous(&mut self) {
        self.active_field = self.active_field.previous();
    }

    /// Returns a mutable reference to the active input field.
    pub fn active_input_mut(&mut self) -> &mut String {
        match self.active_field {
            DirectoryField::Root => &mut self.root_input,
            DirectoryField::Shared => &mut self.shared_input,
            DirectoryField::Shared2023 => &mut self.shared_2023_input,
        }
    }
}

impl FilterState {
    /// Returns `true` if any filter is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.text.is_empty() || self.status.is_some()
    }

    /// Clears all filters.
    pub fn clear(&mut self) {
        self.text.clear();
        self.status = None;
    }

    /// Cycles through status filters.
    pub fn cycle_status(&mut self) {
        self.status = match self.status {
            None => Some(MigrationStatus::Legacy),
            Some(MigrationStatus::Legacy) => Some(MigrationStatus::Partial),
            Some(MigrationStatus::Partial) => Some(MigrationStatus::Migrated),
            Some(MigrationStatus::Migrated) => Some(MigrationStatus::NoModels),
            Some(MigrationStatus::NoModels | _) => None,
        };
    }
}

/// Status message to display in the status bar.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    /// The message text.
    pub text: String,

    /// When the message was created.
    pub timestamp: Instant,

    /// Whether this is an error message.
    pub is_error: bool,
}

impl StatusMessage {
    /// Creates a new info message.
    #[must_use]
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            timestamp: Instant::now(),
            is_error: false,
        }
    }

    /// Creates a new error message.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            timestamp: Instant::now(),
            is_error: true,
        }
    }

    /// Returns `true` if the message should be auto-hidden.
    ///
    /// Messages are hidden after 5 seconds.
    #[must_use]
    pub fn should_hide(&self) -> bool {
        self.timestamp.elapsed().as_secs() > 5
    }
}

/// The main application state.
pub struct App {
    /// The configuration.
    pub config: Config,

    /// The file scanner.
    pub scanner: Scanner,

    /// Cached list of all files (sorted by path).
    files: Vec<FileInfo>,

    /// Current UI mode.
    pub mode: AppMode,

    /// Which panel has focus.
    pub focus: Focus,

    /// File list widget state.
    pub file_list_state: FileListState,

    /// Detail pane widget state.
    pub detail_state: DetailPaneState,

    /// Current filter configuration.
    pub filter: FilterState,

    /// Status message to display.
    pub status: Option<StatusMessage>,

    /// Directory setup input state.
    pub directory_setup: DirectorySetup,

    /// Pending watcher restart path (if needed).
    pending_watcher_restart: Option<Utf8PathBuf>,

    /// Whether the application should quit.
    pub should_quit: bool,

    /// Last scan statistics.
    pub stats: StatsSnapshot,

    /// Terminal size (updated on resize).
    pub terminal_size: Rect,
}

impl App {
    /// Creates a new application with the given configuration and scanner.
    #[must_use]
    pub fn new(config: Config, scanner: Scanner) -> Self {
        let needs_setup = Self::requires_directory_setup(&config);
        let directory_setup = DirectorySetup::from_config(&config);
        let mode = if needs_setup {
            AppMode::DirectorySetup
        } else {
            AppMode::Normal
        };
        let status = if needs_setup {
            Some(StatusMessage::info(
                "Select directories and press Enter to apply",
            ))
        } else {
            None
        };
        Self {
            config,
            scanner,
            files: Vec::new(),
            mode,
            focus: Focus::FileList,
            file_list_state: FileListState::new(),
            detail_state: DetailPaneState::default(),
            filter: FilterState::default(),
            status,
            directory_setup,
            pending_watcher_restart: None,
            should_quit: false,
            stats: StatsSnapshot::default(),
            terminal_size: Rect::default(),
        }
    }

    /// Performs the initial scan.
    ///
    /// # Errors
    ///
    /// Returns an error if the scan fails.
    pub fn initial_scan(&mut self) -> Result<(), TuiError> {
        info!("Performing initial scan");
        let result = self.scanner.scan()?;

        self.stats = result.stats;
        self.refresh_file_list();

        if !result.errors.is_empty() {
            let msg = format!("Scan completed with {} errors", result.errors.len());
            self.status = Some(StatusMessage::error(msg));
        } else {
            let msg = format!("Scanned {} files", self.stats.total);
            self.status = Some(StatusMessage::info(msg));
        }

        Ok(())
    }

    /// Handles a key event and returns the resulting action.
    #[must_use]
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // Global quit handling
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }

        match self.mode {
            AppMode::Normal => self.handle_normal_key(key),
            AppMode::Filtering => self.handle_filter_key(key),
            AppMode::Help => self.handle_help_key(key),
            AppMode::DirectorySetup => self.handle_directory_setup_key(key),
        }
    }

    /// Handles a key event in normal mode.
    fn handle_normal_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('?') => Action::ToggleHelp,
            KeyCode::Char('j') | KeyCode::Down => Action::NextItem,
            KeyCode::Char('k') | KeyCode::Up => Action::PreviousItem,
            KeyCode::Char('g') | KeyCode::Home => Action::FirstItem,
            KeyCode::Char('G') | KeyCode::End => Action::LastItem,
            KeyCode::PageDown => Action::PageDown,
            KeyCode::PageUp => Action::PageUp,
            KeyCode::Tab => Action::ToggleFocus,
            KeyCode::Char('/') => Action::EnterFilterMode,
            KeyCode::Char('f') => Action::CycleStatusFilter,
            KeyCode::Char('r') => Action::Rescan,
            KeyCode::Char('d') => Action::EnterDirectorySetup,
            KeyCode::Esc => {
                if self.filter.is_active() {
                    Action::ClearFilter
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }

    /// Handles a key event in filter mode.
    fn handle_filter_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::ExitFilterMode,
            KeyCode::Enter => {
                self.mode = AppMode::Normal;
                Action::None
            }
            KeyCode::Backspace => {
                self.filter.text.pop();
                Action::SetFilter(self.filter.text.clone())
            }
            KeyCode::Char(c) => {
                self.filter.text.push(c);
                Action::SetFilter(self.filter.text.clone())
            }
            _ => Action::None,
        }
    }

    /// Handles a key event in help mode.
    #[allow(clippy::unused_self)] // Keep &mut self for consistency
    fn handle_help_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q' | '?') => Action::HideHelp,
            _ => Action::None,
        }
    }

    /// Handles a key event in directory setup mode.
    fn handle_directory_setup_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::ExitDirectorySetup,
            KeyCode::Enter => Action::ApplyDirectorySetup,
            KeyCode::Tab => {
                self.directory_setup.focus_next();
                Action::None
            }
            KeyCode::BackTab => {
                self.directory_setup.focus_previous();
                Action::None
            }
            KeyCode::Backspace => {
                self.directory_setup.active_input_mut().pop();
                Action::None
            }
            KeyCode::Char(c) => {
                self.directory_setup.active_input_mut().push(c);
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Handles a mouse event and returns the resulting action.
    #[must_use]
    #[allow(clippy::unused_self)]
    pub fn handle_mouse(&mut self, _event: MouseEvent) -> Action {
        // Basic mouse handling - can be expanded
        Action::None
    }

    /// Updates the application state based on an action.
    #[allow(clippy::match_same_arms)] // Actions are semantically different even if implementation is same
    pub fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,

            Action::NextItem => {
                self.file_list_state.select_next(self.files.len());
            }
            Action::PreviousItem => {
                self.file_list_state.select_previous(self.files.len());
            }
            Action::FirstItem => {
                self.file_list_state.select_first(self.files.len());
            }
            Action::LastItem => {
                self.file_list_state.select_last(self.files.len());
            }
            Action::PageDown => {
                self.file_list_state.page_down(self.files.len());
            }
            Action::PageUp => {
                self.file_list_state.page_up(self.files.len());
            }
            Action::SelectItem(idx) => {
                self.file_list_state.select(idx, self.files.len());
            }

            Action::ToggleFocus => {
                self.focus = self.focus.toggle();
            }
            Action::FocusFileList => {
                self.focus = Focus::FileList;
            }
            Action::FocusDetailPane => {
                self.focus = Focus::DetailPane;
            }

            Action::EnterFilterMode => {
                self.mode = AppMode::Filtering;
            }
            Action::ExitFilterMode => {
                self.mode = AppMode::Normal;
            }
            Action::SetFilter(text) => {
                self.filter.text = text;
                self.apply_filter();
            }
            Action::ClearFilter => {
                self.filter.clear();
                self.file_list_state.clear_filter();
                self.mode = AppMode::Normal;
            }
            Action::CycleStatusFilter => {
                self.filter.cycle_status();
                self.apply_filter();
            }
            Action::SetStatusFilter(status) => {
                self.filter.status = status;
                self.apply_filter();
            }

            Action::Rescan => {
                if let Err(e) = self.rescan() {
                    warn!(error = %e, "Rescan failed");
                    self.status = Some(StatusMessage::error(format!("Rescan failed: {e}")));
                }
            }
            Action::RescanFile(path) => {
                self.rescan_file(&path);
            }

            Action::ToggleHelp => {
                self.mode = if self.mode == AppMode::Help {
                    AppMode::Normal
                } else {
                    AppMode::Help
                };
            }
            Action::ShowHelp => {
                self.mode = AppMode::Help;
            }
            Action::HideHelp => {
                self.mode = AppMode::Normal;
            }

            Action::EnterDirectorySetup => {
                self.directory_setup.refresh_from_config(&self.config);
                self.mode = AppMode::DirectorySetup;
            }
            Action::ExitDirectorySetup => {
                if Self::requires_directory_setup(&self.config) {
                    self.status = Some(StatusMessage::error(
                        "Directory setup required to continue",
                    ));
                } else {
                    self.mode = AppMode::Normal;
                }
            }
            Action::ApplyDirectorySetup => {
                match self.apply_directory_setup() {
                    Ok(()) => {
                        self.mode = AppMode::Normal;
                    }
                    Err(e) => {
                        self.status = Some(StatusMessage::error(format!("{e}")));
                    }
                }
            }

            Action::ShowStatus(text) => {
                self.status = Some(StatusMessage::info(text));
            }
            Action::ClearStatus => {
                self.status = None;
            }

            Action::OpenInEditor | Action::CopyPath => {
                // Not implemented yet
            }

            Action::Render | Action::Tick | Action::None => {}
        }
    }

    /// Handles a tick event (periodic update).
    pub fn tick(&mut self) {
        // Clear stale status messages
        if let Some(ref status) = self.status {
            if status.should_hide() {
                self.status = None;
            }
        }
    }

    /// Returns true if the directory setup should be shown.
    #[must_use]
    pub fn needs_directory_setup(&self) -> bool {
        Self::requires_directory_setup(&self.config)
    }

    /// Returns the pending watcher restart path, if any.
    pub fn take_watcher_restart(&mut self) -> Option<Utf8PathBuf> {
        self.pending_watcher_restart.take()
    }

    /// Performs a full rescan.
    fn rescan(&mut self) -> Result<ScanResult, TuiError> {
        info!("Rescanning files");
        let result = self.scanner.scan()?;
        self.stats = result.stats;
        self.refresh_file_list();

        let msg = format!("Rescanned {} files", self.stats.total);
        self.status = Some(StatusMessage::info(msg));

        Ok(result)
    }

    fn apply_directory_setup(&mut self) -> Result<(), TuiError> {
        let paths = self.parse_directory_inputs()?;

        self.config.scan.root_path = paths.root.clone();
        self.config.scan.shared_path = paths.shared.clone();
        self.config.scan.shared_2023_path = paths.shared_2023.clone();

        if let Some(shared_name) = self.config.scan.shared_path.file_name() {
            self.config.scan.shared_dir = shared_name.to_owned();
        }
        if let Some(shared_2023_name) = self.config.scan.shared_2023_path.file_name() {
            self.config.scan.shared_2023_dir = shared_2023_name.to_owned();
        }

        self.rebuild_scanner()?;
        self.pending_watcher_restart = if self.config.watch.enabled {
            Some(self.config.scan.root_path.clone())
        } else {
            None
        };

        if let Err(e) = self.rescan() {
            self.status = Some(StatusMessage::error(format!("Rescan failed: {e}")));
        } else {
            self.status = Some(StatusMessage::info("Directories updated"));
        }
        Ok(())
    }

    fn parse_directory_inputs(&self) -> Result<DirectoryPaths, TuiError> {
        let root = parse_dir_input("WebApp.Desktop/src", &self.directory_setup.root_input)?;
        let shared = parse_dir_input("shared", &self.directory_setup.shared_input)?;
        let shared_2023 = parse_dir_input("shared_2023", &self.directory_setup.shared_2023_input)?;

        Ok(DirectoryPaths {
            root,
            shared,
            shared_2023,
        })
    }

    fn rebuild_scanner(&mut self) -> Result<(), TuiError> {
        let scanner_config = ScannerConfig::new(&self.config.scan.root_path)
            .with_skip_dirs(&["node_modules", "dist", ".git"]);
        let matcher = ModelPathMatcher::from_scan_config(&self.config.scan);
        self.scanner = Scanner::new_with_matcher(scanner_config, matcher)?;
        Ok(())
    }

    /// Rescans a specific file.
    fn rescan_file(&mut self, path: &Utf8PathBuf) {
        debug!(path = %path, "Rescanning file");
        let results = self.scanner.rescan_files(std::slice::from_ref(path));

        for (p, result) in results {
            if let Err(e) = result {
                warn!(path = %p, error = %e, "Failed to rescan file");
            }
        }

        self.stats = self.scanner.stats();
        self.refresh_file_list();
    }

    /// Refreshes the file list from the scanner cache.
    fn refresh_file_list(&mut self) {
        self.files = self.scanner.cache().all_files();

        // Sort by path for consistent ordering
        self.files.sort_by(|a, b| a.path.cmp(&b.path));

        // Re-apply filter if active
        if self.filter.is_active() {
            self.apply_filter();
        } else if self.file_list_state.selected.is_none() && !self.files.is_empty() {
            self.file_list_state.selected = Some(0);
        }
    }

    /// Applies the current filter to the file list.
    fn apply_filter(&mut self) {
        if !self.filter.is_active() {
            self.file_list_state.clear_filter();
            return;
        }

        let text_lower = self.filter.text.to_lowercase();
        let status_filter = self.filter.status;

        let indices: Vec<usize> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                // Text filter
                let text_match =
                    text_lower.is_empty() || file.path.as_str().to_lowercase().contains(&text_lower);

                // Status filter
                let status_match = status_filter.is_none_or(|s| file.status == s);

                text_match && status_match
            })
            .map(|(i, _)| i)
            .collect();

        self.file_list_state.set_filter(Some(indices));
    }

    /// Returns the currently selected file, if any.
    #[must_use]
    pub fn selected_file(&self) -> Option<&FileInfo> {
        self.file_list_state
            .selected
            .map(|idx| self.file_list_state.actual_index(idx))
            .and_then(|idx| self.files.get(idx))
    }

    /// Returns all files (for rendering).
    #[must_use]
    pub fn files(&self) -> &[FileInfo] {
        &self.files
    }

    /// Returns the total number of files.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns the count of files matching the current filter.
    #[must_use]
    pub fn filtered_count(&self) -> usize {
        self.file_list_state.len(self.files.len())
    }

    /// Updates the terminal size.
    pub fn set_terminal_size(&mut self, size: Rect) {
        self.terminal_size = size;
    }

    /// Handles a file change event from the watcher.
    ///
    /// This method processes file change notifications and triggers
    /// a rescan of the affected file.
    ///
    /// # Arguments
    ///
    /// * `event` - The file change event to handle
    ///
    /// # Returns
    ///
    /// Returns an `Action` to perform, typically `RescanFile` or `None`.
    #[must_use]
    pub fn handle_file_change(&mut self, event: FileEvent) -> Action {
        // Only process TypeScript files
        if !event.is_typescript() {
            debug!(path = %event.path, "Ignoring non-TypeScript file change");
            return Action::None;
        }

        info!(path = %event.path, "File changed, triggering rescan");

        // Show status message
        let file_name = event.file_name().unwrap_or(event.path.as_str());
        self.status = Some(StatusMessage::info(format!("File changed: {file_name}")));

        // Return action to rescan the file
        Action::RescanFile(event.path)
    }
}

#[derive(Debug)]
struct DirectoryPaths {
    root: Utf8PathBuf,
    shared: Utf8PathBuf,
    shared_2023: Utf8PathBuf,
}

fn parse_dir_input(label: &str, input: &str) -> Result<Utf8PathBuf, TuiError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(TuiError::config(format!("{label} path is required")));
    }

    let path = Utf8PathBuf::from(trimmed);
    if !path.exists() {
        return Err(TuiError::config(format!("{label} path not found: {path}")));
    }
    if !path.is_dir() {
        return Err(TuiError::config(format!(
            "{label} path is not a directory: {path}"
        )));
    }

    Ok(path)
}

fn is_valid_dir(path: &Utf8PathBuf) -> bool {
    !path.as_str().is_empty() && path.exists() && path.is_dir()
}

impl App {
    fn requires_directory_setup(config: &Config) -> bool {
        !is_valid_dir(&config.scan.root_path)
            || !is_valid_dir(&config.scan.shared_path)
            || !is_valid_dir(&config.scan.shared_2023_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_mode_default() {
        assert_eq!(AppMode::default(), AppMode::Normal);
    }

    #[test]
    fn test_focus_toggle() {
        assert_eq!(Focus::FileList.toggle(), Focus::DetailPane);
        assert_eq!(Focus::DetailPane.toggle(), Focus::FileList);
    }

    #[test]
    fn test_filter_state_cycle() {
        let mut filter = FilterState::default();
        assert!(filter.status.is_none());

        filter.cycle_status();
        assert_eq!(filter.status, Some(MigrationStatus::Legacy));

        filter.cycle_status();
        assert_eq!(filter.status, Some(MigrationStatus::Partial));

        filter.cycle_status();
        assert_eq!(filter.status, Some(MigrationStatus::Migrated));

        filter.cycle_status();
        assert_eq!(filter.status, Some(MigrationStatus::NoModels));

        filter.cycle_status();
        assert!(filter.status.is_none());
    }

    #[test]
    fn test_filter_state_is_active() {
        let mut filter = FilterState::default();
        assert!(!filter.is_active());

        filter.text = "test".to_owned();
        assert!(filter.is_active());

        filter.clear();
        assert!(!filter.is_active());

        filter.status = Some(MigrationStatus::Legacy);
        assert!(filter.is_active());
    }

    #[test]
    fn test_file_list_state_navigation() {
        let mut state = FileListState::new();
        state.visible_height = 10;

        // With 0 files
        state.select_next(0);
        assert!(state.selected.is_none());

        // With 5 files
        state.select_next(5);
        assert_eq!(state.selected, Some(0));

        state.select_next(5);
        assert_eq!(state.selected, Some(1));

        state.select_last(5);
        assert_eq!(state.selected, Some(4));

        state.select_next(5);
        assert_eq!(state.selected, Some(0)); // Wrap

        state.select_previous(5);
        assert_eq!(state.selected, Some(4)); // Wrap back

        state.select_first(5);
        assert_eq!(state.selected, Some(0));
    }

    #[test]
    fn test_status_message() {
        let msg = StatusMessage::info("Test message");
        assert!(!msg.is_error);
        assert!(!msg.should_hide()); // Just created, shouldn't hide yet

        let err = StatusMessage::error("Error!");
        assert!(err.is_error);
    }
}
