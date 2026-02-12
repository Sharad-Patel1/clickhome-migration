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
use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use ch_core::{Config, FileInfo, MigrationStatus};
use ch_graph::{
    DependencyGraph, DependencyGraphBuilder, GraphComparator, GraphDiff, GraphPlanner,
    MigrationPlan, PlannerConfig, build_inventory,
};
use ch_scanner::{ScanConfig as ScannerConfig, Scanner, StatsSnapshot};
use ch_ts_parser::ModelPathMatcher;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use tracing::info;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

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

    /// Build dependency graph and migration plan artifacts.
    Graph {
        /// Output directory for generated graph and plan artifacts.
        #[arg(long, default_value = "./graph-artifacts")]
        output_dir: Utf8PathBuf,

        /// Snapshot detail level used for artifact payloads.
        #[arg(long, value_enum, default_value_t = GraphSnapshotMode::Minimal)]
        snapshot_mode: GraphSnapshotMode,

        /// Output format selection.
        #[arg(long, value_enum, default_value_t = GraphOutputFormat::All)]
        format: GraphOutputFormat,

        /// Optional cap on emitted migration plan steps.
        #[arg(long)]
        max_steps: Option<usize>,
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

/// Snapshot detail mode for graph artifacts.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum GraphSnapshotMode {
    /// Emit compact summary-focused artifacts.
    Minimal,
    /// Emit expanded artifacts with node/edge and evidence detail.
    Full,
}

/// Graph artifact output format selection.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum GraphOutputFormat {
    /// Emit JSON artifacts.
    Json,
    /// Emit Graphviz DOT artifacts.
    Dot,
    /// Emit Markdown artifacts.
    Md,
    /// Emit all supported artifact formats.
    All,
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
/// * `is_tui` - Whether the command starts the TUI (`watch`)
fn init_tracing(verbose: bool, no_color: bool, is_tui: bool) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = if verbose { "debug" } else { "info" };
        EnvFilter::new(format!("{level},hyper=warn,mio=warn,notify=warn"))
    });

    if is_tui {
        let (writer, guard) = create_watch_log_writer();
        tracing_subscriber::registry()
            .with(fmt::layer().with_target(false).with_ansi(false).with_writer(writer))
            .with(filter)
            .init();
        Some(guard)
    } else {
        // Check if colors should be disabled (flag or NO_COLOR env var)
        let use_ansi = !no_color && std::env::var("NO_COLOR").is_err();

        tracing_subscriber::registry()
            .with(fmt::layer().with_target(false).with_ansi(use_ansi))
            .with(filter)
            .init();
        None
    }
}

/// Creates a non-blocking file writer for watch-mode tracing logs.
fn create_watch_log_writer() -> (NonBlocking, WorkerGuard) {
    let requested_path = std::env::var_os("CH_MIGRATE_LOG_FILE")
        .map_or_else(|| PathBuf::from("/tmp/ch-migrate-watch.log"), PathBuf::from);

    let (mut directory, mut file_name) = split_log_path(&requested_path);
    if std::fs::create_dir_all(&directory).is_err() {
        directory = PathBuf::from("/tmp");
        "ch-migrate-watch.log".clone_into(&mut file_name);
        let _ = std::fs::create_dir_all(&directory);
    }

    let appender = tracing_appender::rolling::never(directory, file_name);
    tracing_appender::non_blocking(appender)
}

/// Splits a path into `(directory, file_name)` suitable for rolling file appenders.
fn split_log_path(path: &Path) -> (PathBuf, String) {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("ch-migrate-watch.log")
        .to_owned();

    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    (directory, file_name)
}

/// Builds a [`Config`] from CLI arguments.
///
/// Validates that the path exists and is a directory.
///
/// # Errors
///
/// Returns an error if the path is not provided, doesn't exist, or isn't a directory.
fn build_config(cli: &Cli, require_shared_paths: bool) -> color_eyre::Result<Config> {
    let path = cli.path.clone().unwrap_or_else(|| Utf8PathBuf::from("./WebApp.Desktop/src"));

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
    config.scan.shared_path =
        cli.shared_path.clone().unwrap_or_else(|| config.scan.root_path.join("app").join("shared"));
    config.scan.shared_2023_path = cli
        .shared_2023_path
        .clone()
        .unwrap_or_else(|| config.scan.root_path.join("app").join("shared_2023"));

    // Set app_path: use CLI arg or default to ./WebApp.Desktop/src/app
    config.scan.app_path =
        cli.app_path.clone().unwrap_or_else(|| config.scan.root_path.join("app"));

    if let Some(name) = config.scan.shared_path.file_name() {
        config.scan.shared_dir = name.to_owned();
    }
    if let Some(name) = config.scan.shared_2023_path.file_name() {
        config.scan.shared_2023_dir = name.to_owned();
    }
    config.editor.editor.clone_from(&cli.editor);

    validate_dir(&config.scan.shared_path, "shared", require_shared_paths)?;
    validate_dir(&config.scan.shared_2023_path, "shared_2023", require_shared_paths)?;
    // app_path is always required since we scan it for model consumers
    validate_dir(&config.scan.app_path, "app", true)?;

    Ok(config)
}

fn validate_dir(path: &Utf8PathBuf, label: &str, required: bool) -> color_eyre::Result<()> {
    if path.as_str().is_empty() {
        if required {
            return Err(color_eyre::eyre::eyre!("{label} path is required but missing."));
        }
        return Ok(());
    }

    if !path.exists() {
        if required {
            return Err(color_eyre::eyre::eyre!("{label} path does not exist: {path}"));
        }
        return Ok(());
    }

    if !path.is_dir() {
        return Err(color_eyre::eyre::eyre!("{label} path is not a directory: {path}"));
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
fn create_scanner(config: &Config, use_registry: bool) -> color_eyre::Result<Scanner> {
    // Use app_path for scanning (not root_path) to restrict to application code only
    let mut scanner_config =
        ScannerConfig::new(&config.scan.app_path).with_skip_dirs(&["node_modules", "dist", ".git"]);
    if use_registry {
        scanner_config = scanner_config
            .with_shared_paths(&config.scan.shared_path, &config.scan.shared_2023_path);
    }
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

    let scanner = create_scanner(config, false)?;
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

    let scanner = create_scanner(&config, false)?;

    let mut config = config;
    config.watch.enabled = !no_watch;

    // Handle SIGTERM for graceful shutdown on Unix
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

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

    let scanner = create_scanner(config, false)?;
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

/// Builds dependency graph + migration plan artifacts.
fn run_graph(
    config: &Config,
    output_dir: &Utf8PathBuf,
    snapshot_mode: GraphSnapshotMode,
    format: GraphOutputFormat,
    max_steps: Option<usize>,
) -> color_eyre::Result<()> {
    if matches!(max_steps, Some(0)) {
        return Err(color_eyre::eyre::eyre!("--max-steps must be greater than 0 when provided."));
    }

    if output_dir.exists() && !output_dir.is_dir() {
        return Err(color_eyre::eyre::eyre!(
            "output directory path is not a directory: {output_dir}"
        ));
    }

    std::fs::create_dir_all(output_dir.as_std_path())?;

    info!(
        app_path = %config.scan.app_path,
        shared_path = %config.scan.shared_path,
        shared_2023_path = %config.scan.shared_2023_path,
        output_dir = %output_dir,
        snapshot_mode = ?snapshot_mode,
        format = ?format,
        max_steps = ?max_steps,
        "Building graph artifacts"
    );

    let scanner = create_scanner(config, true)?;
    let result = scanner.scan()?;
    let all_files = scanner.cache().all_files();

    let inventory = build_inventory(scanner.registry());
    let graph = DependencyGraphBuilder::new()
        .with_source_roots(vec![
            config.scan.root_path.clone(),
            config.scan.app_path.clone(),
            config.scan.shared_path.clone(),
            config.scan.shared_2023_path.clone(),
        ])
        .build(&inventory, &all_files);
    let diff = GraphComparator::new().compare(&inventory, &all_files);
    let planner =
        GraphPlanner::with_config(PlannerConfig { max_steps, ..PlannerConfig::default() });
    let plan = planner.plan(&graph, &diff);

    let written = export_graph_artifacts(output_dir, snapshot_mode, format, &graph, &diff, &plan)?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "Graph artifacts written to: {output_dir}")?;
    for path in &written {
        writeln!(out, "  {}", path.as_str())?;
    }
    writeln!(
        out,
        "Graph summary: {} nodes, {} edges, {} plan steps",
        graph.node_count(),
        graph.edge_count(),
        plan.steps.len()
    )?;

    if !result.errors.is_empty() {
        let stderr = std::io::stderr();
        let mut err = stderr.lock();
        writeln!(
            err,
            "Scan completed with {} non-fatal file errors while building graph artifacts.",
            result.errors.len()
        )?;
    }

    Ok(())
}

fn export_graph_artifacts(
    output_dir: &Utf8PathBuf,
    snapshot_mode: GraphSnapshotMode,
    format: GraphOutputFormat,
    graph: &DependencyGraph,
    diff: &GraphDiff,
    plan: &MigrationPlan,
) -> color_eyre::Result<Vec<Utf8PathBuf>> {
    let mut written = Vec::new();

    if matches!(format, GraphOutputFormat::Json | GraphOutputFormat::All) {
        written.extend(write_json_artifacts(output_dir, snapshot_mode, graph, diff, plan)?);
    }
    if matches!(format, GraphOutputFormat::Dot | GraphOutputFormat::All) {
        written.extend(write_dot_artifacts(output_dir, snapshot_mode, graph, plan)?);
    }
    if matches!(format, GraphOutputFormat::Md | GraphOutputFormat::All) {
        written.extend(write_markdown_artifacts(output_dir, snapshot_mode, graph, diff, plan)?);
    }

    written.sort();
    Ok(written)
}

fn write_json_artifacts(
    output_dir: &Utf8PathBuf,
    snapshot_mode: GraphSnapshotMode,
    graph: &DependencyGraph,
    diff: &GraphDiff,
    plan: &MigrationPlan,
) -> color_eyre::Result<Vec<Utf8PathBuf>> {
    let graph_payload = graph_json_payload(graph, diff, snapshot_mode);
    let plan_payload = migration_plan_json_payload(plan, snapshot_mode);

    let graph_path = output_dir.join("graph.json");
    let plan_path = output_dir.join("migration-plan.json");

    let graph_bytes = serde_json::to_vec_pretty(&graph_payload)?;
    let plan_bytes = serde_json::to_vec_pretty(&plan_payload)?;

    std::fs::write(graph_path.as_std_path(), graph_bytes)?;
    std::fs::write(plan_path.as_std_path(), plan_bytes)?;

    Ok(vec![graph_path, plan_path])
}

fn write_dot_artifacts(
    output_dir: &Utf8PathBuf,
    snapshot_mode: GraphSnapshotMode,
    graph: &DependencyGraph,
    plan: &MigrationPlan,
) -> color_eyre::Result<Vec<Utf8PathBuf>> {
    let graph_dot = render_graph_dot(graph, snapshot_mode);
    let plan_dot = render_plan_dot(plan, snapshot_mode);

    let graph_path = output_dir.join("graph.dot");
    let plan_path = output_dir.join("migration-plan.dot");
    std::fs::write(graph_path.as_std_path(), graph_dot)?;
    std::fs::write(plan_path.as_std_path(), plan_dot)?;

    Ok(vec![graph_path, plan_path])
}

fn write_markdown_artifacts(
    output_dir: &Utf8PathBuf,
    snapshot_mode: GraphSnapshotMode,
    graph: &DependencyGraph,
    diff: &GraphDiff,
    plan: &MigrationPlan,
) -> color_eyre::Result<Vec<Utf8PathBuf>> {
    let graph_md = render_graph_markdown(graph, diff, snapshot_mode);
    let plan_md = render_plan_markdown(plan, snapshot_mode);

    let graph_path = output_dir.join("graph.md");
    let plan_path = output_dir.join("migration-plan.md");
    std::fs::write(graph_path.as_std_path(), graph_md)?;
    std::fs::write(plan_path.as_std_path(), plan_md)?;

    Ok(vec![graph_path, plan_path])
}

fn graph_json_payload(
    graph: &DependencyGraph,
    diff: &GraphDiff,
    snapshot_mode: GraphSnapshotMode,
) -> serde_json::Value {
    let metadata = graph.metadata();
    let counts = &metadata.counts;
    let mut edge_kind_counts: Vec<_> = counts
        .edges_by_kind
        .iter()
        .map(|entry| json!({"kind": format!("{:?}", entry.kind), "count": entry.count}))
        .collect();
    edge_kind_counts.sort_by(|left, right| {
        value_string(left, "kind")
            .cmp(&value_string(right, "kind"))
            .then_with(|| value_usize(left, "count").cmp(&value_usize(right, "count")))
    });

    let graph_section = match snapshot_mode {
        GraphSnapshotMode::Minimal => {
            let mut node_ids: Vec<_> = graph
                .graph()
                .node_indices()
                .map(|node_index| graph.graph()[node_index].node_id.clone())
                .collect();
            node_ids.sort();

            json!({
                "node_ids": node_ids,
                "edges": collect_graph_edges(graph, false),
            })
        }
        GraphSnapshotMode::Full => json!({
            "nodes": collect_graph_nodes(graph, true),
            "edges": collect_graph_edges(graph, true),
        }),
    };

    let diff_section = match snapshot_mode {
        GraphSnapshotMode::Minimal => json!({
            "counts": {
                "matched": diff.counts.matched,
                "low_confidence": diff.counts.low_confidence,
                "no_match": diff.counts.no_match,
                "residuals": diff.counts.residuals,
            }
        }),
        GraphSnapshotMode::Full => json!({
            "counts": {
                "matched": diff.counts.matched,
                "low_confidence": diff.counts.low_confidence,
                "no_match": diff.counts.no_match,
                "residuals": diff.counts.residuals,
            },
            "mappings": diff.mappings.iter().map(|mapping| {
                json!({
                    "legacy_canonical_id": mapping.legacy_canonical_id,
                    "legacy_symbol": mapping.legacy_symbol,
                    "modern_canonical_id": mapping.modern_canonical_id,
                    "modern_symbol": mapping.modern_symbol,
                    "confidence_bps": mapping.confidence_bps,
                    "status": format!("{:?}", mapping.status),
                    "reasons": mapping.reasons.iter().map(|reason| json!({
                        "kind": format!("{:?}", reason.kind),
                        "weight_bps": reason.weight_bps,
                        "details": reason.details,
                    })).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
            "residual_legacy_usages": diff.residual_legacy_usages.iter().map(|residual| {
                json!({
                    "file_path": residual.file_path,
                    "legacy_symbol": residual.legacy_symbol,
                    "suggested_modern_symbol": residual.suggested_modern_symbol,
                    "confidence_bps": residual.confidence_bps,
                    "reasons": residual.reasons.iter().map(|reason| json!({
                        "kind": format!("{:?}", reason.kind),
                        "weight_bps": reason.weight_bps,
                        "details": reason.details,
                    })).collect::<Vec<_>>(),
                    "anchors": residual.anchors,
                })
            }).collect::<Vec<_>>(),
        }),
    };

    json!({
        "snapshot_mode": snapshot_mode_label(snapshot_mode),
        "metadata": {
            "counts": {
                "total_nodes": counts.total_nodes,
                "total_edges": counts.total_edges,
                "interface_nodes": counts.interface_nodes,
                "model_nodes": counts.model_nodes,
                "service_nodes": counts.service_nodes,
                "file_nodes": counts.file_nodes,
                "symbol_nodes": counts.symbol_nodes,
                "edges_by_kind": edge_kind_counts,
            },
            "source_roots": metadata.source_roots,
            "parser_metadata": {
                "parser_version": metadata.parser_metadata.parser_version,
                "relation_query_version": metadata.parser_metadata.relation_query_version,
            },
        },
        "graph": graph_section,
        "diff": diff_section,
    })
}

fn migration_plan_json_payload(
    plan: &MigrationPlan,
    snapshot_mode: GraphSnapshotMode,
) -> serde_json::Value {
    let steps = match snapshot_mode {
        GraphSnapshotMode::Minimal => plan
            .steps
            .iter()
            .map(|step| {
                json!({
                    "step_id": step.step_id,
                    "order": step.order,
                    "component_id": step.component_id,
                    "prerequisites": step.prerequisites,
                    "risk_score_bps": step.risk_score_bps,
                    "impacted_file_count": step.impacted_files.len(),
                })
            })
            .collect::<Vec<_>>(),
        GraphSnapshotMode::Full => plan
            .steps
            .iter()
            .map(|step| {
                json!({
                    "step_id": step.step_id,
                    "order": step.order,
                    "component_id": step.component_id,
                    "node_ids": step.node_ids,
                    "prerequisites": step.prerequisites,
                    "impacted_files": step.impacted_files,
                    "suggested_replacements": step.suggested_replacements.iter().map(|replacement| json!({
                        "legacy_symbol": replacement.legacy_symbol,
                        "modern_symbol": replacement.modern_symbol,
                        "confidence_bps": replacement.confidence_bps,
                    })).collect::<Vec<_>>(),
                    "evidence_refs": step.evidence_refs.iter().map(|evidence| json!({
                        "relation": format!("{:?}", evidence.relation),
                        "source": evidence.source,
                        "target": evidence.target,
                        "anchors": evidence.anchors,
                    })).collect::<Vec<_>>(),
                    "risk_score_bps": step.risk_score_bps,
                    "risk_breakdown": {
                        "components": step.risk_breakdown.components.iter().map(|component| json!({
                            "kind": format!("{:?}", component.kind),
                            "raw_value": component.raw_value,
                            "normalized_bps": component.normalized_bps,
                            "weighted_bps": component.weighted_bps,
                        })).collect::<Vec<_>>(),
                    },
                })
            })
            .collect::<Vec<_>>(),
    };

    json!({
        "snapshot_mode": snapshot_mode_label(snapshot_mode),
        "counts": {
            "total_components": plan.counts.total_components,
            "emitted_steps": plan.counts.emitted_steps,
            "truncated_components": plan.counts.truncated_components,
        },
        "steps": steps,
    })
}

fn collect_graph_nodes(graph: &DependencyGraph, include_details: bool) -> Vec<serde_json::Value> {
    let mut nodes: Vec<_> = graph
        .graph()
        .node_indices()
        .map(|node_index| {
            let node = &graph.graph()[node_index];
            let mut value = json!({
                "node_id": node.node_id,
                "kind": format!("{:?}", node.kind),
            });

            if include_details {
                if let serde_json::Value::Object(ref mut map) = value {
                    map.insert(
                        "source_classification".to_owned(),
                        json!(format!("{:?}", node.source_classification)),
                    );
                    map.insert(
                        "source".to_owned(),
                        json!(node.source.map(|source| format!("{source:?}"))),
                    );
                    map.insert("canonical_id".to_owned(), json!(node.canonical_id));
                    map.insert("model_name".to_owned(), json!(node.model_name));
                    map.insert("symbol_name".to_owned(), json!(node.symbol_name));
                    map.insert(
                        "category".to_owned(),
                        json!(node.category.map(|category| format!("{category:?}"))),
                    );
                    map.insert("export_name".to_owned(), json!(node.export_name));
                    map.insert(
                        "definition_path".to_owned(),
                        json!(node.definition_path.as_ref().map(ToString::to_string)),
                    );
                    map.insert("file_path".to_owned(), json!(node.file_path));
                }
            }

            value
        })
        .collect();

    nodes.sort_by_key(|left| value_string(left, "node_id"));
    nodes
}

fn collect_graph_edges(graph: &DependencyGraph, include_evidence: bool) -> Vec<serde_json::Value> {
    let mut edges = Vec::new();

    for edge_index in graph.graph().edge_indices() {
        let Some((source_index, target_index)) = graph.graph().edge_endpoints(edge_index) else {
            continue;
        };
        let Some(edge) = graph.graph().edge_weight(edge_index) else {
            continue;
        };

        let source = graph.graph()[source_index].node_id.clone();
        let target = graph.graph()[target_index].node_id.clone();
        let kind = format!("{:?}", edge.kind);
        let mut value = json!({
            "source_node_id": source,
            "target_node_id": target,
            "kind": kind,
            "evidence_count": edge.evidence.len(),
        });

        if include_evidence {
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert("evidence".to_owned(), json!(edge.evidence));
            }
        }

        edges.push(value);
    }

    edges.sort_by(|left, right| {
        value_string(left, "source_node_id")
            .cmp(&value_string(right, "source_node_id"))
            .then_with(|| {
                value_string(left, "target_node_id").cmp(&value_string(right, "target_node_id"))
            })
            .then_with(|| value_string(left, "kind").cmp(&value_string(right, "kind")))
    });
    edges
}

fn render_graph_dot(graph: &DependencyGraph, snapshot_mode: GraphSnapshotMode) -> String {
    use std::fmt::Write as _;

    let mut output = String::from("digraph dependency_graph {\n  rankdir=LR;\n");
    for node in collect_graph_nodes(graph, snapshot_mode == GraphSnapshotMode::Full) {
        let node_id = value_string(&node, "node_id");
        let label = if snapshot_mode == GraphSnapshotMode::Full {
            let kind = value_string(&node, "kind");
            format!("{node_id}\\n{kind}")
        } else {
            node_id.clone()
        };
        let _ =
            writeln!(output, "  \"{}\" [label=\"{}\"];", dot_escape(&node_id), dot_escape(&label));
    }

    for edge in collect_graph_edges(graph, false) {
        let source = value_string(&edge, "source_node_id");
        let target = value_string(&edge, "target_node_id");
        let kind = value_string(&edge, "kind");
        let _ = writeln!(
            output,
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            dot_escape(&source),
            dot_escape(&target),
            dot_escape(&kind)
        );
    }

    output.push_str("}\n");
    output
}

fn render_plan_dot(plan: &MigrationPlan, snapshot_mode: GraphSnapshotMode) -> String {
    use std::fmt::Write as _;

    let mut output = String::from("digraph migration_plan {\n  rankdir=LR;\n");
    for step in &plan.steps {
        let label = if snapshot_mode == GraphSnapshotMode::Full {
            format!("{}\\nrisk={}bps", step.step_id, step.risk_score_bps)
        } else {
            step.step_id.clone()
        };
        let _ = writeln!(
            output,
            "  \"{}\" [label=\"{}\"];",
            dot_escape(&step.step_id),
            dot_escape(&label)
        );
    }

    for step in &plan.steps {
        for prerequisite in &step.prerequisites {
            let _ = writeln!(
                output,
                "  \"{}\" -> \"{}\";",
                dot_escape(prerequisite),
                dot_escape(&step.step_id)
            );
        }
    }

    output.push_str("}\n");
    output
}

fn render_graph_markdown(
    graph: &DependencyGraph,
    diff: &GraphDiff,
    snapshot_mode: GraphSnapshotMode,
) -> String {
    use std::fmt::Write as _;

    let counts = &graph.metadata().counts;
    let mut output = String::new();
    let _ = writeln!(output, "# Dependency Graph");
    let _ = writeln!(output);
    let _ = writeln!(output, "- Snapshot mode: `{}`", snapshot_mode_label(snapshot_mode));
    let _ = writeln!(output, "- Total nodes: `{}`", counts.total_nodes);
    let _ = writeln!(output, "- Total edges: `{}`", counts.total_edges);
    let _ = writeln!(output, "- Mappings matched: `{}`", diff.counts.matched);
    let _ = writeln!(output, "- Mappings low-confidence: `{}`", diff.counts.low_confidence);
    let _ = writeln!(output, "- Mappings no-match: `{}`", diff.counts.no_match);
    let _ = writeln!(output, "- Residual legacy usages: `{}`", diff.counts.residuals);

    if snapshot_mode == GraphSnapshotMode::Full {
        let _ = writeln!(output);
        let _ = writeln!(output, "## Nodes");
        let _ = writeln!(output);
        let _ = writeln!(output, "| Node ID | Kind |");
        let _ = writeln!(output, "| --- | --- |");
        for node in collect_graph_nodes(graph, false) {
            let _ = writeln!(
                output,
                "| `{}` | `{}` |",
                value_string(&node, "node_id"),
                value_string(&node, "kind")
            );
        }
    }

    output
}

fn render_plan_markdown(plan: &MigrationPlan, snapshot_mode: GraphSnapshotMode) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# Migration Plan");
    let _ = writeln!(output);
    let _ = writeln!(output, "- Snapshot mode: `{}`", snapshot_mode_label(snapshot_mode));
    let _ = writeln!(output, "- Total components: `{}`", plan.counts.total_components);
    let _ = writeln!(output, "- Emitted steps: `{}`", plan.counts.emitted_steps);
    let _ = writeln!(output, "- Truncated components: `{}`", plan.counts.truncated_components);
    let _ = writeln!(output);
    let _ = writeln!(output, "| Step | Order | Prerequisites | Risk (bps) |");
    let _ = writeln!(output, "| --- | --- | --- | --- |");
    for step in &plan.steps {
        let prerequisites = if step.prerequisites.is_empty() {
            "-".to_owned()
        } else {
            step.prerequisites.join(", ")
        };
        let _ = writeln!(
            output,
            "| `{}` | `{}` | `{}` | `{}` |",
            step.step_id, step.order, prerequisites, step.risk_score_bps
        );
    }

    if snapshot_mode == GraphSnapshotMode::Full {
        for step in &plan.steps {
            let _ = writeln!(output);
            let _ = writeln!(output, "## {}", step.step_id);
            let _ = writeln!(output);
            let _ = writeln!(output, "- Component: `{}`", step.component_id);
            let _ = writeln!(output, "- Node count: `{}`", step.node_ids.len());
            let _ = writeln!(output, "- Impacted files: `{}`", step.impacted_files.len());
            let _ = writeln!(
                output,
                "- Suggested replacements: `{}`",
                step.suggested_replacements.len()
            );
            let _ = writeln!(output, "- Evidence refs: `{}`", step.evidence_refs.len());
        }
    }

    output
}

fn dot_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

const fn snapshot_mode_label(mode: GraphSnapshotMode) -> &'static str {
    match mode {
        GraphSnapshotMode::Minimal => "minimal",
        GraphSnapshotMode::Full => "full",
    }
}

fn value_string(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(serde_json::Value::as_str).map(ToOwned::to_owned).unwrap_or_default()
}

fn value_usize(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|raw| usize::try_from(raw).ok())
        .unwrap_or_default()
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
    let _ = writeln!(handle, "  Legacy:           {} (need migration)", stats.legacy);
    let _ = writeln!(handle, "  Partial:          {} (in progress)", stats.partial);
    let _ = writeln!(handle, "  Migrated:         {} (complete)", stats.migrated);
    let _ = writeln!(handle, "  No models:        {} (no action needed)", stats.no_models);
    let _ = writeln!(handle, "  Errors:           {}", stats.errors);
    let _ = writeln!(handle);
    let _ = writeln!(handle, "Migration progress: {:.1}%", stats.progress_percent());
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

    // 3. Initialize tracing (watch logs go to file to avoid TUI redraw corruption)
    let is_watch_mode = matches!(&cli.command, Commands::Watch { .. });
    let _log_guard = init_tracing(cli.verbose, cli.no_color, is_watch_mode);

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
        Commands::Graph { output_dir, snapshot_mode, format, max_steps } => {
            let config = build_config(&cli, true)?;
            run_graph(&config, output_dir, *snapshot_mode, *format, *max_steps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn test_graph_command_defaults() {
        let parsed = Cli::try_parse_from(["ch-migrate", "graph"]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            match cli.command {
                Commands::Graph { output_dir, snapshot_mode, format, max_steps } => {
                    assert_eq!(output_dir, Utf8PathBuf::from("./graph-artifacts"));
                    assert_eq!(snapshot_mode, GraphSnapshotMode::Minimal);
                    assert_eq!(format, GraphOutputFormat::All);
                    assert!(max_steps.is_none());
                }
                _ => assert!(false),
            }
        }
    }

    #[test]
    fn test_graph_command_accepts_custom_values_and_global_paths() {
        let parsed = Cli::try_parse_from([
            "ch-migrate",
            "--path",
            "/tmp/project/src",
            "--shared-path",
            "/tmp/project/src/app/shared",
            "--shared-2023-path",
            "/tmp/project/src/app/shared_2023",
            "--app-path",
            "/tmp/project/src/app",
            "graph",
            "--output-dir",
            "/tmp/project/out",
            "--snapshot-mode",
            "full",
            "--format",
            "md",
            "--max-steps",
            "7",
        ]);
        assert!(parsed.is_ok());

        if let Ok(cli) = parsed {
            assert_eq!(cli.path, Some(Utf8PathBuf::from("/tmp/project/src")));
            assert_eq!(cli.shared_path, Some(Utf8PathBuf::from("/tmp/project/src/app/shared")));
            assert_eq!(
                cli.shared_2023_path,
                Some(Utf8PathBuf::from("/tmp/project/src/app/shared_2023"))
            );
            assert_eq!(cli.app_path, Some(Utf8PathBuf::from("/tmp/project/src/app")));

            if let Commands::Graph { output_dir, snapshot_mode, format, max_steps } = cli.command {
                assert_eq!(output_dir, Utf8PathBuf::from("/tmp/project/out"));
                assert_eq!(snapshot_mode, GraphSnapshotMode::Full);
                assert_eq!(format, GraphOutputFormat::Md);
                assert_eq!(max_steps, Some(7));
            } else {
                assert!(false);
            }
        }
    }

    #[test]
    fn test_graph_help_mentions_graph_specific_options() {
        let mut command = Cli::command();
        let graph = command.find_subcommand_mut("graph");
        assert!(graph.is_some());

        if let Some(graph) = graph {
            let mut help = Vec::new();
            let write_result = graph.write_long_help(&mut help);
            assert!(write_result.is_ok());
            let help_text = String::from_utf8_lossy(&help);
            assert!(help_text.contains("--output-dir"));
            assert!(help_text.contains("--snapshot-mode"));
            assert!(help_text.contains("--format"));
            assert!(help_text.contains("--max-steps"));
            assert!(help_text.contains("--shared-path"));
            assert!(help_text.contains("--shared-2023-path"));
            assert!(help_text.contains("--app-path"));
        }
    }

    #[test]
    fn test_existing_commands_still_parse() {
        assert!(Cli::try_parse_from(["ch-migrate", "scan"]).is_ok());
        assert!(Cli::try_parse_from(["ch-migrate", "watch"]).is_ok());
        assert!(Cli::try_parse_from(["ch-migrate", "report"]).is_ok());
    }

    #[test]
    fn test_run_graph_writes_all_artifacts() {
        let root = create_temp_project("graph-all");
        let output_dir = root.join("out-all");
        let config = fixture_config(&root);
        assert!(config.is_ok());

        if let Ok(config) = config {
            let run_result = run_graph(
                &config,
                &output_dir,
                GraphSnapshotMode::Full,
                GraphOutputFormat::All,
                Some(8),
            );
            assert!(run_result.is_ok());

            assert!(output_dir.join("graph.json").exists());
            assert!(output_dir.join("migration-plan.json").exists());
            assert!(output_dir.join("graph.dot").exists());
            assert!(output_dir.join("migration-plan.dot").exists());
            assert!(output_dir.join("graph.md").exists());
            assert!(output_dir.join("migration-plan.md").exists());
        }

        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn test_run_graph_respects_json_only_format_selection() {
        let root = create_temp_project("graph-json");
        let output_dir = root.join("out-json");
        let config = fixture_config(&root);
        assert!(config.is_ok());

        if let Ok(config) = config {
            let run_result = run_graph(
                &config,
                &output_dir,
                GraphSnapshotMode::Minimal,
                GraphOutputFormat::Json,
                Some(4),
            );
            assert!(run_result.is_ok());

            assert!(output_dir.join("graph.json").exists());
            assert!(output_dir.join("migration-plan.json").exists());
            assert!(!output_dir.join("graph.dot").exists());
            assert!(!output_dir.join("migration-plan.dot").exists());
            assert!(!output_dir.join("graph.md").exists());
            assert!(!output_dir.join("migration-plan.md").exists());
        }

        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    fn fixture_config(root: &Utf8PathBuf) -> color_eyre::Result<Config> {
        let cli = Cli {
            command: Commands::Graph {
                output_dir: root.join("out"),
                snapshot_mode: GraphSnapshotMode::Minimal,
                format: GraphOutputFormat::All,
                max_steps: None,
            },
            path: Some(root.clone()),
            shared_path: None,
            shared_2023_path: None,
            app_path: None,
            verbose: false,
            no_color: true,
            editor: None,
        };
        build_config(&cli, true)
    }

    fn create_temp_project(suffix: &str) -> Utf8PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root_std = std::env::temp_dir().join(format!("ch-migrate-cli-{suffix}-{nanos}"));
        let root = Utf8PathBuf::from_path_buf(root_std)
            .unwrap_or_else(|_| Utf8PathBuf::from(format!("/tmp/ch-migrate-cli-{suffix}-{nanos}")));

        write_fixture_file(
            &root.join("app/shared/interfaces.ts"),
            "export interface OrderModel { id: string; }\n",
        );
        write_fixture_file(
            &root.join("app/shared/models/order.ts"),
            "export class OrderModel {}\nexport class OrderCodeGen {}\n",
        );
        write_fixture_file(
            &root.join("app/shared_2023/interfaces.ts"),
            "export interface OrderModel { id: string; }\n",
        );
        write_fixture_file(
            &root.join("app/shared_2023/models/order.ts"),
            "export class OrderModel {}\nexport class OrderCodeGen {}\n",
        );
        write_fixture_file(
            &root.join("app/features/order-consumer.ts"),
            r#"import { OrderModel } from "../shared/models/order";
import { OrderModel as OrderModel2023 } from "../shared_2023/models/order";

export class OrderConsumer {
  legacy: OrderModel;
  modern: OrderModel2023;
}
"#,
        );

        root
    }

    fn write_fixture_file(path: &Utf8PathBuf, contents: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent.as_std_path());
        }
        let _ = std::fs::write(path.as_std_path(), contents);
    }
}
