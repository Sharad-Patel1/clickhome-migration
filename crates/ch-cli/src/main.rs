//! CLI entry point for the ch-migration tool.
//!
//! This binary provides the command-line interface for migrating
//! TypeScript models from `shared` to `shared_2023` in the `ClickHome` codebase.
//!
//! # Usage
//!
//! ```bash
//! ch-migrate [OPTIONS] <COMMAND>
//!
//! # Scan and show summary
//! ch-migrate scan --path /path/to/WebApp.Desktop/src
//!
//! # Interactive TUI with file watching
//! ch-migrate watch --path /path/to/WebApp.Desktop/src
//!
//! # Generate JSON report
//! ch-migrate report --format json --output report.json
//! ```

#![deny(clippy::all)]
#![warn(missing_docs)]

use std::io::Write;

use camino::Utf8PathBuf;
use ch_core::{Config, FileInfo, MigrationStatus};
use ch_scanner::{ScanConfig as ScannerConfig, Scanner, StatsSnapshot};
use ch_ts_parser::ModelPathMatcher;
use clap::{Parser, Subcommand, ValueEnum};
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// =============================================================================
// CLI ARGUMENT TYPES
// =============================================================================

/// CLI tool for migrating TypeScript models from `shared/` to `shared_2023/`.
///
/// Scans the `ClickHome` `WebApp.Desktop` source directory to identify files
/// that need migration and tracks progress.
#[derive(Parser)]
#[command(name = "ch-migrate", version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Command to execute.
    #[command(subcommand)]
    command: Commands,

    /// Path to WebApp.Desktop/src directory.
    ///
    /// Defaults to `./WebApp.Desktop/src` if not specified.
    #[arg(short, long, global = true, env = "CH_MIGRATE_PATH")]
    path: Option<Utf8PathBuf>,

    /// Absolute path to legacy shared directory.
    ///
    /// Defaults to `./WebApp.Desktop/src/app/shared` if not specified.
    #[arg(long, global = true, env = "CH_MIGRATE_SHARED_PATH")]
    shared_path: Option<Utf8PathBuf>,

    /// Absolute path to `shared_2023` directory.
    ///
    /// Defaults to `./WebApp.Desktop/src/app/shared_2023` if not specified.
    #[arg(long, global = true, env = "CH_MIGRATE_SHARED_2023_PATH")]
    shared_2023_path: Option<Utf8PathBuf>,

    /// Path to app directory to scan for model consumers.
    ///
    /// Defaults to `./WebApp.Desktop/src/app` if not specified. This restricts scanning
    /// to only the application code directory, excluding shared model definitions.
    #[arg(long, global = true, env = "CH_MIGRATE_APP_PATH")]
    app_path: Option<Utf8PathBuf>,

    /// Enable verbose logging (debug level).
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Disable colored output.
    #[arg(long, global = true)]
    no_color: bool,

    /// Editor to use for opening files (overrides $EDITOR).
    #[arg(long, global = true, env = "CH_MIGRATE_EDITOR")]
    editor: Option<String>,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Scan codebase and display migration status summary.
    Scan {
        /// Show detailed file list.
        #[arg(short, long)]
        detailed: bool,
    },

    /// Start interactive TUI with live file watching.
    Watch {
        /// Disable file watching (static view).
        #[arg(long)]
        no_watch: bool,
    },

    /// Generate migration report.
    Report {
        /// Output format.
        #[arg(short, long, value_enum, default_value_t = ReportFormat::Json)]
        format: ReportFormat,

        /// Output file (defaults to stdout).
        #[arg(short, long)]
        output: Option<Utf8PathBuf>,
    },
}

/// Report output format.
#[derive(Clone, Copy, ValueEnum)]
enum ReportFormat {
    /// JSON format.
    Json,
    /// CSV format.
    Csv,
}

// =============================================================================
// INITIALIZATION FUNCTIONS
// =============================================================================

/// Initializes the tracing subscriber for logging.
///
/// Respects the `RUST_LOG` environment variable if set. Otherwise, uses
/// `debug` level if `--verbose` is set, or `info` level by default.
/// Noisy crates like `hyper` and `mio` are filtered to `warn` level.
///
/// # Arguments
///
/// * `verbose` - Enable debug-level logging
/// * `no_color` - Disable ANSI colors in output
fn init_tracing(verbose: bool, no_color: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = if verbose { "debug" } else { "info" };
        EnvFilter::new(format!("{level},hyper=warn,mio=warn,notify=warn"))
    });

    // Check if colors should be disabled (flag or NO_COLOR env var)
    let use_ansi = !no_color && std::env::var("NO_COLOR").is_err();

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false).with_ansi(use_ansi))
        .with(filter)
        .init();
}

/// Builds a [`Config`] from CLI arguments.
///
/// Validates that the path exists and is a directory.
///
/// # Errors
///
/// Returns an error if the path is not provided, doesn't exist, or isn't a directory.
fn build_config(cli: &Cli, require_shared_paths: bool) -> color_eyre::Result<Config> {
    let path = cli
        .path
        .clone()
        .unwrap_or_else(|| Utf8PathBuf::from("./WebApp.Desktop/src"));

    // Validate path exists
    if !path.exists() {
        return Err(color_eyre::eyre::eyre!("Path does not exist: {}", path));
    }

    // Validate path is a directory
    if !path.is_dir() {
        return Err(color_eyre::eyre::eyre!("Path is not a directory: {}", path));
    }

    let mut config = Config::default();
    config.scan.root_path = path;
    config.scan.shared_path = cli
        .shared_path
        .clone()
        .unwrap_or_else(|| config.scan.root_path.join("app").join("shared"));
    config.scan.shared_2023_path = cli
        .shared_2023_path
        .clone()
        .unwrap_or_else(|| config.scan.root_path.join("app").join("shared_2023"));

    // Set app_path: use CLI arg or default to ./WebApp.Desktop/src/app
    config.scan.app_path = cli
        .app_path
        .clone()
        .unwrap_or_else(|| config.scan.root_path.join("app"));

    if let Some(name) = config.scan.shared_path.file_name() {
        config.scan.shared_dir = name.to_owned();
    }
    if let Some(name) = config.scan.shared_2023_path.file_name() {
        config.scan.shared_2023_dir = name.to_owned();
    }
    config.editor.editor.clone_from(&cli.editor);

    validate_dir(&config.scan.shared_path, "shared", require_shared_paths)?;
    validate_dir(
        &config.scan.shared_2023_path,
        "shared_2023",
        require_shared_paths,
    )?;
    // app_path is always required since we scan it for model consumers
    validate_dir(&config.scan.app_path, "app", true)?;

    Ok(config)
}

fn validate_dir(path: &Utf8PathBuf, label: &str, required: bool) -> color_eyre::Result<()> {
    if path.as_str().is_empty() {
        if required {
            return Err(color_eyre::eyre::eyre!(
                "{label} path is required but missing."
            ));
        }
        return Ok(());
    }

    if !path.exists() {
        if required {
            return Err(color_eyre::eyre::eyre!(
                "{label} path does not exist: {path}"
            ));
        }
        return Ok(());
    }

    if !path.is_dir() {
        return Err(color_eyre::eyre::eyre!(
            "{label} path is not a directory: {path}"
        ));
    }

    Ok(())
}

/// Creates a [`Scanner`] from the configuration.
///
/// Uses `app_path` as the scan root to restrict scanning to only application
/// code, excluding shared model definition directories.
///
/// # Errors
///
/// Returns an error if the scanner cannot be created.
fn create_scanner(config: &Config) -> color_eyre::Result<Scanner> {
    // Use app_path for scanning (not root_path) to restrict to application code only
    let scanner_config =
        ScannerConfig::new(&config.scan.app_path).with_skip_dirs(&["node_modules", "dist", ".git"]);
    let matcher = ModelPathMatcher::from_scan_config(&config.scan);

    Scanner::new_with_matcher(scanner_config, matcher)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to create scanner: {}", e))
}

// =============================================================================
// COMMAND IMPLEMENTATIONS
// =============================================================================

/// Runs a one-shot scan with summary output.
///
/// # Arguments
///
/// * `config` - The application configuration
/// * `detailed` - Whether to show detailed file list
///
/// # Errors
///
/// Returns an error if scanning fails.
fn run_scan(config: &Config, detailed: bool) -> color_eyre::Result<()> {
    info!(app_path = %config.scan.app_path, "Starting scan");

    let scanner = create_scanner(config)?;
    let result = scanner.scan()?;

    print_stats_summary(&result.stats);

    if detailed {
        print_detailed_file_list(&scanner);
    }

    // Print any errors encountered
    if !result.errors.is_empty() {
        let stderr = std::io::stderr();
        let mut handle = stderr.lock();
        writeln!(handle)?;
        writeln!(handle, "Errors ({}):", result.errors.len())?;
        for (path, error) in &result.errors {
            writeln!(handle, "  {path} - {error}")?;
        }
    }

    Ok(())
}

/// Runs the interactive TUI with optional file watching.
///
/// # Arguments
///
/// * `config` - The application configuration
/// * `no_watch` - Whether to disable file watching
///
/// # Errors
///
/// Returns an error if the TUI fails.
async fn run_watch(config: Config, no_watch: bool) -> color_eyre::Result<()> {
    info!(app_path = %config.scan.app_path, watch = !no_watch, "Starting TUI");

    let scanner = create_scanner(&config)?;

    let mut config = config;
    config.watch.enabled = !no_watch;

    // Handle SIGTERM for graceful shutdown on Unix
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())?;

        tokio::select! {
            result = ch_tui::run(config, scanner) => {
                result.map_err(|e| color_eyre::eyre::eyre!("TUI error: {}", e))?;
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down");
            }
        }
    }

    #[cfg(not(unix))]
    {
        ch_tui::run(config, scanner)
            .await
            .map_err(|e| color_eyre::eyre::eyre!("TUI error: {}", e))?;
    }

    Ok(())
}

/// Generates a migration report in the specified format.
///
/// # Arguments
///
/// * `config` - The application configuration
/// * `format` - Output format (JSON or CSV)
/// * `output` - Output file path (stdout if None)
///
/// # Errors
///
/// Returns an error if scanning or writing fails.
fn run_report(
    config: &Config,
    format: ReportFormat,
    output: Option<Utf8PathBuf>,
) -> color_eyre::Result<()> {
    info!(app_path = %config.scan.app_path, "Generating report");

    let scanner = create_scanner(config)?;
    let result = scanner.scan()?;

    let all_files = scanner.cache().all_files();

    let content = match format {
        ReportFormat::Json => generate_json_report(&result.stats, &all_files)?,
        ReportFormat::Csv => generate_csv_report(&all_files),
    };

    if let Some(output_path) = output {
        std::fs::write(output_path.as_std_path(), &content)?;
        info!(path = %output_path, "Report written");
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        write!(handle, "{content}")?;
    }

    Ok(())
}

// =============================================================================
// OUTPUT HELPERS
// =============================================================================

/// Prints a summary of scan statistics.
fn print_stats_summary(stats: &StatsSnapshot) {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    let _ = writeln!(handle);
    let _ = writeln!(handle, "Migration Status Summary");
    let _ = writeln!(handle, "========================");
    let _ = writeln!(handle);
    let _ = writeln!(handle, "Total files scanned: {}", stats.total);
    let _ = writeln!(
        handle,
        "  Legacy:           {} (need migration)",
        stats.legacy
    );
    let _ = writeln!(
        handle,
        "  Partial:          {} (in progress)",
        stats.partial
    );
    let _ = writeln!(handle, "  Migrated:         {} (complete)", stats.migrated);
    let _ = writeln!(
        handle,
        "  No models:        {} (no action needed)",
        stats.no_models
    );
    let _ = writeln!(handle, "  Errors:           {}", stats.errors);
    let _ = writeln!(handle);
    let _ = writeln!(
        handle,
        "Migration progress: {:.1}%",
        stats.progress_percent()
    );
    let _ = writeln!(handle, "Files needing work: {}", stats.needs_migration());
}

/// Prints a detailed list of files needing migration.
fn print_detailed_file_list(scanner: &Scanner) {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    let legacy_files = scanner.files_with_status(MigrationStatus::Legacy);
    let partial_files = scanner.files_with_status(MigrationStatus::Partial);

    if !legacy_files.is_empty() {
        let _ = writeln!(handle);
        let _ = writeln!(handle, "Legacy files ({}):", legacy_files.len());
        for file in &legacy_files {
            let _ = writeln!(handle, "  {}", file.path);
        }
    }

    if !partial_files.is_empty() {
        let _ = writeln!(handle);
        let _ = writeln!(handle, "Partial files ({}):", partial_files.len());
        for file in &partial_files {
            let _ = writeln!(handle, "  {}", file.path);
        }
    }
}

/// Generates a JSON report.
fn generate_json_report(stats: &StatsSnapshot, files: &[FileInfo]) -> color_eyre::Result<String> {
    #[derive(serde::Serialize)]
    struct Report<'a> {
        stats: &'a StatsSnapshot,
        files: &'a [FileInfo],
    }

    let report = Report { stats, files };
    serde_json::to_string_pretty(&report)
        .map_err(|e| color_eyre::eyre::eyre!("Failed to serialize JSON: {}", e))
}

/// Generates a CSV report.
fn generate_csv_report(files: &[FileInfo]) -> String {
    use std::fmt::Write;

    let mut output = String::from("path,status,import_count,legacy_imports,migrated_imports\n");

    for file in files {
        let legacy_count = file.legacy_imports().count();
        let migrated_count = file.migrated_imports().count();
        let escaped_path = escape_csv(file.path.as_str());
        let status = file.status.label();
        let import_count = file.import_count();

        // Use write! to avoid extra allocation from format!
        let _ = writeln!(
            output,
            "{escaped_path},{status},{import_count},{legacy_count},{migrated_count}"
        );
    }

    output
}

/// Escapes a string for CSV output.
fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

// =============================================================================
// MAIN ENTRY POINT
// =============================================================================

/// Application entry point.
#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    // 1. Install color-eyre FIRST (before any potential panics)
    color_eyre::install()?;

    // 2. Parse CLI arguments
    let cli = Cli::parse();

    // 3. Initialize tracing (handles --no-color for log output)
    init_tracing(cli.verbose, cli.no_color);

    // 5. Route to appropriate command
    match &cli.command {
        Commands::Scan { detailed } => {
            let config = build_config(&cli, true)?;
            run_scan(&config, *detailed)
        }
        Commands::Watch { no_watch } => {
            let config = build_config(&cli, false)?;
            run_watch(config, *no_watch).await
        }
        Commands::Report { format, output } => {
            let config = build_config(&cli, true)?;
            run_report(&config, *format, output.clone())
        }
    }
}
