//! User actions for the TUI.
//!
//! This module defines the [`Action`] enum representing all user-initiated
//! actions that can be performed in the TUI. Actions are the result of
//! processing input events (key presses, mouse clicks) and are used to
//! update application state.
//!
//! # Action Flow
//!
//! ```text
//! Key/Mouse Event → Component → Action → App State Update
//! ```

use ch_core::MigrationStatus;

/// User-initiated actions in the TUI.
///
/// Actions represent commands that modify application state. They are
/// produced by components in response to input events and processed
/// by the application's update loop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Action {
    // =========================================================================
    // Navigation
    // =========================================================================
    /// Move selection to the next item.
    NextItem,

    /// Move selection to the previous item.
    PreviousItem,

    /// Move selection to the first item.
    FirstItem,

    /// Move selection to the last item.
    LastItem,

    /// Move selection down by one page.
    PageDown,

    /// Move selection up by one page.
    PageUp,

    /// Select a specific item by index.
    SelectItem(usize),

    // =========================================================================
    // Focus Management
    // =========================================================================
    /// Toggle focus between panels.
    ToggleFocus,

    /// Focus the file list panel.
    FocusFileList,

    /// Focus the detail pane.
    FocusDetailPane,

    // =========================================================================
    // Filtering
    // =========================================================================
    /// Enter filter mode (start typing filter).
    EnterFilterMode,

    /// Exit filter mode without applying.
    ExitFilterMode,

    /// Update the filter text.
    SetFilter(String),

    /// Clear the current filter.
    ClearFilter,

    /// Cycle through status filters (All → Legacy → Partial → Migrated → All).
    CycleStatusFilter,

    /// Set a specific status filter.
    SetStatusFilter(Option<MigrationStatus>),

    // =========================================================================
    // File Operations
    // =========================================================================
    /// Trigger a rescan of all files.
    Rescan,

    /// Rescan a specific file.
    RescanFile(camino::Utf8PathBuf),

    /// Start a background streaming scan.
    ///
    /// This initiates a new scan that streams results incrementally.
    StartStreamingScan,

    /// Open the selected file in the default editor.
    OpenInEditor,

    /// Copy the selected file path to clipboard.
    CopyPath,

    // =========================================================================
    // UI State
    // =========================================================================
    /// Toggle the help panel.
    ToggleHelp,

    /// Show the help panel.
    ShowHelp,

    /// Hide the help panel.
    HideHelp,

    /// Show a status message.
    ShowStatus(String),

    /// Clear the status message.
    ClearStatus,

    // =========================================================================
    // Directory Setup
    // =========================================================================
    /// Enter directory setup mode.
    EnterDirectorySetup,

    /// Exit directory setup mode.
    ExitDirectorySetup,

    /// Apply directory setup changes.
    ApplyDirectorySetup,

    // =========================================================================
    // Application Control
    // =========================================================================
    /// Quit the application.
    Quit,

    /// Render the UI.
    Render,

    /// Tick (periodic update).
    Tick,

    /// No operation (used for event handling that doesn't produce an action).
    #[default]
    None,
}

impl Action {
    /// Returns `true` if this action requires a re-render.
    #[must_use]
    pub const fn needs_render(&self) -> bool {
        !matches!(self, Self::None | Self::Tick)
    }

    /// Returns `true` if this is a navigation action.
    #[must_use]
    pub const fn is_navigation(&self) -> bool {
        matches!(
            self,
            Self::NextItem
                | Self::PreviousItem
                | Self::FirstItem
                | Self::LastItem
                | Self::PageDown
                | Self::PageUp
                | Self::SelectItem(_)
        )
    }

    /// Returns `true` if this is a filter-related action.
    #[must_use]
    pub const fn is_filter(&self) -> bool {
        matches!(
            self,
            Self::EnterFilterMode
                | Self::ExitFilterMode
                | Self::SetFilter(_)
                | Self::ClearFilter
                | Self::CycleStatusFilter
                | Self::SetStatusFilter(_)
        )
    }

    /// Returns `true` if this action modifies the filter state.
    #[must_use]
    pub const fn modifies_filter(&self) -> bool {
        matches!(
            self,
            Self::SetFilter(_) | Self::ClearFilter | Self::SetStatusFilter(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_needs_render() {
        assert!(Action::NextItem.needs_render());
        assert!(Action::ToggleHelp.needs_render());
        assert!(!Action::None.needs_render());
        assert!(!Action::Tick.needs_render());
    }

    #[test]
    fn test_action_is_navigation() {
        assert!(Action::NextItem.is_navigation());
        assert!(Action::PreviousItem.is_navigation());
        assert!(Action::FirstItem.is_navigation());
        assert!(Action::PageDown.is_navigation());
        assert!(Action::SelectItem(5).is_navigation());

        assert!(!Action::Quit.is_navigation());
        assert!(!Action::ToggleHelp.is_navigation());
    }

    #[test]
    fn test_action_is_filter() {
        assert!(Action::EnterFilterMode.is_filter());
        assert!(Action::SetFilter("test".to_owned()).is_filter());
        assert!(Action::CycleStatusFilter.is_filter());

        assert!(!Action::NextItem.is_filter());
        assert!(!Action::Quit.is_filter());
    }

    #[test]
    fn test_action_modifies_filter() {
        assert!(Action::SetFilter("test".to_owned()).modifies_filter());
        assert!(Action::ClearFilter.modifies_filter());
        assert!(Action::SetStatusFilter(Some(MigrationStatus::Legacy)).modifies_filter());

        assert!(!Action::EnterFilterMode.modifies_filter());
        assert!(!Action::CycleStatusFilter.modifies_filter());
    }

    #[test]
    fn test_action_default() {
        assert_eq!(Action::default(), Action::None);
    }
}
