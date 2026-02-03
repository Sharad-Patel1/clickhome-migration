//! Configuration structures for the ch-migration tool.
//!
//! This module provides configuration types for all components of the application:
//!
//! - [`ScanConfig`] - Scanner settings (paths, extensions, parallelism)
//! - [`WatchConfig`] - File watcher settings (debouncing, recursion)
//! - [`TuiConfig`] - Terminal UI settings (tick rate, colors)
//! - [`Config`] - Root configuration combining all settings
//!
//! All configuration types implement [`Default`] with sensible values for the
//! `ClickHome` project structure.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

/// Color scheme for the TUI.
///
/// Controls the visual appearance of the terminal interface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ColorScheme {
    /// Automatically detect based on terminal settings.
    #[default]
    Auto,
    /// Light color scheme (dark text on light background).
    Light,
    /// Dark color scheme (light text on dark background).
    Dark,
}

/// Configuration for the file scanner.
///
/// Controls how the scanner traverses the filesystem and which files to analyze.
///
/// # Examples
///
/// ```
/// use ch_core::ScanConfig;
///
/// let config = ScanConfig::default();
/// assert_eq!(config.shared_dir, "shared");
/// assert_eq!(config.shared_2023_dir, "shared_2023");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanConfig {
    /// Root path to the WebApp.Desktop/src directory.
    pub root_path: Utf8PathBuf,

    /// Name of the legacy shared directory (typically "shared").
    pub shared_dir: String,

    /// Name of the new shared directory (typically `shared_2023`).
    pub shared_2023_dir: String,

    /// Subdirectory containing model files (typically "models").
    pub models_subdir: String,

    /// Subdirectory containing codegen files (typically "models/codegen").
    pub codegen_subdir: String,

    /// File extensions to scan (e.g., `.ts`, `.tsx`).
    pub file_extensions: Vec<String>,

    /// Additional glob patterns to ignore during scanning.
    pub ignore_patterns: Vec<String>,

    /// Maximum number of parallel scanning jobs.
    /// `None` means use all available CPU cores.
    pub max_parallel_jobs: Option<usize>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            root_path: Utf8PathBuf::new(),
            shared_dir: "shared".to_owned(),
            shared_2023_dir: "shared_2023".to_owned(),
            models_subdir: "models".to_owned(),
            codegen_subdir: "models/codegen".to_owned(),
            file_extensions: vec![".ts".to_owned(), ".tsx".to_owned()],
            ignore_patterns: vec![
                "node_modules".to_owned(),
                "dist".to_owned(),
                "*.spec.ts".to_owned(),
                "*.test.ts".to_owned(),
            ],
            max_parallel_jobs: None,
        }
    }
}

/// Configuration for the file watcher.
///
/// Controls how file changes are detected and debounced.
///
/// # Examples
///
/// ```
/// use ch_core::WatchConfig;
///
/// let config = WatchConfig::default();
/// assert_eq!(config.debounce_ms, 100);
/// assert!(config.recursive);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchConfig {
    /// Debounce window in milliseconds.
    ///
    /// Multiple file changes within this window are batched into a single event.
    pub debounce_ms: u64,

    /// Whether to watch subdirectories recursively.
    pub recursive: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 100,
            recursive: true,
        }
    }
}

/// Configuration for the terminal user interface.
///
/// Controls the visual and behavioral aspects of the TUI.
///
/// # Examples
///
/// ```
/// use ch_core::{TuiConfig, ColorScheme};
///
/// let config = TuiConfig::default();
/// assert_eq!(config.tick_rate_ms, 250);
/// assert_eq!(config.color_scheme, ColorScheme::Auto);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    /// UI refresh rate in milliseconds.
    ///
    /// Lower values provide smoother animations but use more CPU.
    pub tick_rate_ms: u64,

    /// Whether to show hidden files in the file list.
    pub show_hidden: bool,

    /// Color scheme for the interface.
    pub color_scheme: ColorScheme,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            tick_rate_ms: 250,
            show_hidden: false,
            color_scheme: ColorScheme::Auto,
        }
    }
}

/// Root configuration for the ch-migration tool.
///
/// Combines all component configurations into a single structure that can be
/// loaded from a configuration file or constructed programmatically.
///
/// # Examples
///
/// ```
/// use ch_core::Config;
///
/// // Create with defaults
/// let config = Config::default();
///
/// // Serialize to JSON
/// let json = serde_json::to_string_pretty(&config).unwrap();
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Scanner configuration.
    pub scan: ScanConfig,

    /// File watcher configuration.
    pub watch: WatchConfig,

    /// Terminal UI configuration.
    pub tui: TuiConfig,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_config_defaults() {
        let config = ScanConfig::default();
        assert_eq!(config.shared_dir, "shared");
        assert_eq!(config.shared_2023_dir, "shared_2023");
        assert_eq!(config.models_subdir, "models");
        assert_eq!(config.file_extensions, vec![".ts", ".tsx"]);
    }

    #[test]
    fn test_watch_config_defaults() {
        let config = WatchConfig::default();
        assert_eq!(config.debounce_ms, 100);
        assert!(config.recursive);
    }

    #[test]
    fn test_tui_config_defaults() {
        let config = TuiConfig::default();
        assert_eq!(config.tick_rate_ms, 250);
        assert!(!config.show_hidden);
        assert_eq!(config.color_scheme, ColorScheme::Auto);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn test_config_deserialize_with_missing_fields() {
        let json = r#"{"scan": {"shared_dir": "custom_shared"}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.scan.shared_dir, "custom_shared");
        // Other fields should have defaults
        assert_eq!(config.scan.shared_2023_dir, "shared_2023");
        assert_eq!(config.watch.debounce_ms, 100);
    }

    #[test]
    fn test_color_scheme_serialization() {
        assert_eq!(
            serde_json::to_string(&ColorScheme::Auto).unwrap(),
            r#""auto""#
        );
        assert_eq!(
            serde_json::to_string(&ColorScheme::Dark).unwrap(),
            r#""dark""#
        );
        assert_eq!(
            serde_json::to_string(&ColorScheme::Light).unwrap(),
            r#""light""#
        );
    }
}
