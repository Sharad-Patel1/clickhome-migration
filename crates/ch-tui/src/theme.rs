//! Theme and styling for the TUI.
//!
//! This module provides the [`Theme`] struct for managing colors and styles
//! throughout the terminal interface. It supports both dark and light color
//! schemes.
//!
//! # Example
//!
//! ```
//! use ch_tui::Theme;
//! use ch_core::MigrationStatus;
//!
//! let theme = Theme::dark();
//! let style = theme.status_style(MigrationStatus::Legacy);
//! ```

use ch_core::{ColorScheme, MigrationStatus};
use ratatui::style::{Color, Modifier, Style};

/// Theme configuration for the TUI.
///
/// Contains all colors and styles used throughout the interface.
/// Use [`Theme::dark()`] or [`Theme::light()`] to get predefined themes,
/// or [`Theme::from_scheme()`] to create a theme based on configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    // =========================================================================
    // Status Colors
    // =========================================================================
    /// Foreground color for legacy (needs migration) status.
    pub legacy_fg: Color,

    /// Foreground color for migrated status.
    pub migrated_fg: Color,

    /// Foreground color for partial migration status.
    pub partial_fg: Color,

    /// Foreground color for files with no model imports.
    pub no_models_fg: Color,

    // =========================================================================
    // Selection Colors
    // =========================================================================
    /// Background color for selected items.
    pub selected_bg: Color,

    /// Foreground color for selected items.
    pub selected_fg: Color,

    // =========================================================================
    // Base Colors
    // =========================================================================
    /// Primary foreground color.
    pub fg: Color,

    /// Primary background color.
    pub bg: Color,

    /// Dimmed/secondary text color.
    pub dimmed_fg: Color,

    /// Accent color for highlights.
    pub accent: Color,

    /// Error/warning color.
    pub error_fg: Color,

    // =========================================================================
    // Border Styles
    // =========================================================================
    /// Style for normal borders.
    pub border_style: Style,

    /// Style for focused borders.
    pub focused_border_style: Style,

    // =========================================================================
    // Component Styles
    // =========================================================================
    /// Style for highlighted/selected items.
    pub highlight_style: Style,

    /// Style for the header bar.
    pub header_style: Style,

    /// Style for the status bar.
    pub status_bar_style: Style,
}

impl Theme {
    /// Creates a dark theme (light text on dark background).
    ///
    /// This is the default theme, optimized for dark terminal backgrounds.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            // Status colors
            legacy_fg: Color::Rgb(255, 100, 100),    // Soft red
            migrated_fg: Color::Rgb(100, 255, 100),  // Soft green
            partial_fg: Color::Rgb(255, 200, 100),   // Soft yellow/orange
            no_models_fg: Color::Rgb(128, 128, 128), // Gray

            // Selection colors
            selected_bg: Color::Rgb(60, 60, 80),
            selected_fg: Color::White,

            // Base colors
            fg: Color::Rgb(220, 220, 220),
            bg: Color::Reset,
            dimmed_fg: Color::Rgb(128, 128, 128),
            accent: Color::Rgb(100, 150, 255), // Soft blue
            error_fg: Color::Rgb(255, 80, 80),

            // Border styles
            border_style: Style::default().fg(Color::Rgb(80, 80, 100)),
            focused_border_style: Style::default().fg(Color::Rgb(100, 150, 255)),

            // Component styles
            highlight_style: Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(60, 60, 80))
                .add_modifier(Modifier::BOLD),
            header_style: Style::default()
                .fg(Color::Rgb(100, 150, 255))
                .add_modifier(Modifier::BOLD),
            status_bar_style: Style::default()
                .fg(Color::Rgb(180, 180, 180))
                .bg(Color::Rgb(40, 40, 50)),
        }
    }

    /// Creates a light theme (dark text on light background).
    ///
    /// Optimized for light terminal backgrounds.
    #[must_use]
    pub fn light() -> Self {
        Self {
            // Status colors
            legacy_fg: Color::Rgb(180, 50, 50),      // Dark red
            migrated_fg: Color::Rgb(50, 150, 50),    // Dark green
            partial_fg: Color::Rgb(180, 130, 50),    // Dark yellow/orange
            no_models_fg: Color::Rgb(100, 100, 100), // Dark gray

            // Selection colors
            selected_bg: Color::Rgb(200, 200, 220),
            selected_fg: Color::Black,

            // Base colors
            fg: Color::Rgb(30, 30, 30),
            bg: Color::Reset,
            dimmed_fg: Color::Rgb(100, 100, 100),
            accent: Color::Rgb(50, 100, 200), // Dark blue
            error_fg: Color::Rgb(180, 50, 50),

            // Border styles
            border_style: Style::default().fg(Color::Rgb(150, 150, 170)),
            focused_border_style: Style::default().fg(Color::Rgb(50, 100, 200)),

            // Component styles
            highlight_style: Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(200, 200, 220))
                .add_modifier(Modifier::BOLD),
            header_style: Style::default()
                .fg(Color::Rgb(50, 100, 200))
                .add_modifier(Modifier::BOLD),
            status_bar_style: Style::default()
                .fg(Color::Rgb(60, 60, 60))
                .bg(Color::Rgb(220, 220, 230)),
        }
    }

    /// Creates a theme from a [`ColorScheme`] configuration.
    ///
    /// If the scheme is [`ColorScheme::Auto`], defaults to dark theme.
    #[must_use]
    pub fn from_scheme(scheme: ColorScheme) -> Self {
        match scheme {
            ColorScheme::Light => Self::light(),
            ColorScheme::Dark | ColorScheme::Auto | _ => Self::dark(),
        }
    }

    /// Returns the style for a given migration status.
    #[must_use]
    pub fn status_style(&self, status: MigrationStatus) -> Style {
        let color = self.status_color(status);
        Style::default().fg(color)
    }

    /// Returns the color for a given migration status.
    #[must_use]
    pub const fn status_color(&self, status: MigrationStatus) -> Color {
        match status {
            MigrationStatus::Legacy => self.legacy_fg,
            MigrationStatus::Migrated => self.migrated_fg,
            MigrationStatus::Partial => self.partial_fg,
            MigrationStatus::NoModels | _ => self.no_models_fg,
        }
    }

    /// Returns the status indicator character for a migration status.
    #[must_use]
    pub const fn status_indicator(status: MigrationStatus) -> &'static str {
        match status {
            MigrationStatus::Legacy => "[L]",
            MigrationStatus::Migrated => "[M]",
            MigrationStatus::Partial => "[P]",
            MigrationStatus::NoModels | _ => "[-]",
        }
    }

    /// Returns a style with the base foreground color.
    #[must_use]
    pub fn base_style(&self) -> Style {
        Style::default().fg(self.fg)
    }

    /// Returns a style for dimmed/secondary text.
    #[must_use]
    pub fn dimmed_style(&self) -> Style {
        Style::default().fg(self.dimmed_fg)
    }

    /// Returns a style for accent/highlighted text.
    #[must_use]
    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Returns a style for error text.
    #[must_use]
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error_fg)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_dark() {
        let theme = Theme::dark();
        assert_eq!(theme.fg, Color::Rgb(220, 220, 220));
    }

    #[test]
    fn test_theme_light() {
        let theme = Theme::light();
        assert_eq!(theme.fg, Color::Rgb(30, 30, 30));
    }

    #[test]
    fn test_theme_from_scheme() {
        let dark = Theme::from_scheme(ColorScheme::Dark);
        let light = Theme::from_scheme(ColorScheme::Light);
        let auto = Theme::from_scheme(ColorScheme::Auto);

        assert_eq!(dark, Theme::dark());
        assert_eq!(light, Theme::light());
        assert_eq!(auto, Theme::dark()); // Auto defaults to dark
    }

    #[test]
    fn test_status_color() {
        let theme = Theme::dark();

        assert_eq!(theme.status_color(MigrationStatus::Legacy), theme.legacy_fg);
        assert_eq!(
            theme.status_color(MigrationStatus::Migrated),
            theme.migrated_fg
        );
        assert_eq!(
            theme.status_color(MigrationStatus::Partial),
            theme.partial_fg
        );
        assert_eq!(
            theme.status_color(MigrationStatus::NoModels),
            theme.no_models_fg
        );
    }

    #[test]
    fn test_status_indicator() {
        assert_eq!(Theme::status_indicator(MigrationStatus::Legacy), "[L]");
        assert_eq!(Theme::status_indicator(MigrationStatus::Migrated), "[M]");
        assert_eq!(Theme::status_indicator(MigrationStatus::Partial), "[P]");
        assert_eq!(Theme::status_indicator(MigrationStatus::NoModels), "[-]");
    }

    #[test]
    fn test_theme_default() {
        assert_eq!(Theme::default(), Theme::dark());
    }
}
