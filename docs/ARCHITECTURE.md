# Architecture

This document describes the high-level architecture of `ch-migration`, a TUI application for migrating TypeScript models from the legacy `shared` directory to `shared_2023` in the ClickHome enterprise web application.

## Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              ch-cli (Binary)                                │
│                     CLI entry point, argument parsing                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              ch-tui (Library)                               │
│              Ratatui components, event loop, UI state management            │
└─────────────────────────────────────────────────────────────────────────────┘
                          │                       │
              ┌───────────┘                       └───────────┐
              ▼                                               ▼
┌──────────────────────────────┐             ┌──────────────────────────────┐
│      ch-scanner (Library)    │             │      ch-watcher (Library)    │
│   Parallel file discovery    │             │   File change detection      │
│   and analysis caching       │             │   and event streaming        │
└──────────────────────────────┘             └──────────────────────────────┘
              │                                               │
              ▼                                               │
┌──────────────────────────────┐                              │
│    ch-ts-parser (Library)    │                              │
│   Tree-sitter TypeScript     │                              │
│   parsing, import detection  │                              │
└──────────────────────────────┘                              │
              │                                               │
              └───────────────────┬───────────────────────────┘
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                             ch-core (Library)                               │
│        Shared types, error handling, configuration, type aliases            │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Design Principles

1. **Composition over Inheritance** - All crates use struct composition and trait implementations rather than inheritance hierarchies.

2. **Pure Functions** - Business logic is implemented as pure functions where possible, with side effects isolated to the edges of the system.

3. **Functional/Procedural Style** - Prefer data transformations via iterators and functional combinators over classic OOP patterns.

4. **Fail Fast, Recover Gracefully** - Use `Result` types throughout; errors bubble up to be handled at appropriate boundaries.

5. **Zero-Copy Where Possible** - Use arena allocation (bumpalo) and string interning to minimize allocations during parsing.

## Crate Responsibilities

### ch-core

**Purpose**: Foundation crate providing shared types and utilities used across the workspace.

**Key Components**:
- `error.rs` - Error types using `thiserror` for ergonomic error handling
- `config.rs` - Configuration structures for CLI options and scan settings
- `types.rs` - Domain types: `FileInfo`, `ModelReference`, `ImportInfo`, `MigrationStatus`
- `hash.rs` - Type aliases for `FxHashMap`/`FxHashSet` (faster than std for string keys)

**Dependencies**: Minimal - only `thiserror`, `serde`, `rustc-hash`, `camino`, `smallvec`

**Design Notes**:
- All types implement `Debug`, `Clone`, and where appropriate `Serialize`/`Deserialize`
- Uses `camino::Utf8Path` for guaranteed UTF-8 path handling
- Exports type aliases to enforce consistent hashing across the workspace

### ch-ts-parser

**Purpose**: Incremental TypeScript parsing using tree-sitter for import and model detection.

**Key Components**:
- `parser.rs` - Tree-sitter parser initialization and management
- `import.rs` - Import statement extraction (static and dynamic imports)
- `model_ref.rs` - Model/interface reference detection within files
- `query.rs` - Pre-compiled tree-sitter queries for performance
- `arena.rs` - Bumpalo arena integration for efficient AST storage

**Dependencies**: `tree-sitter`, `tree-sitter-typescript`, `bumpalo`, `bumpalo-herd`

**Design Notes**:

```
TypeScript Import Patterns Detected:
────────────────────────────────────
import { Model } from '../shared/models/model-name'
import { Model } from '../shared_2023/models/model-name'
import type { Interface } from '../shared/interfaces'
import * as Models from '../shared/models'
const m = await import('../shared/models/model-name')
```

The parser uses tree-sitter's incremental parsing capability:

```rust
// On file change, update the existing tree rather than re-parsing
tree.edit(&InputEdit {
    start_byte,
    old_end_byte,
    new_end_byte,
    start_position,
    old_end_position,
    new_end_position,
});
let new_tree = parser.parse(new_source, Some(&old_tree));
```

**Arena Allocation Strategy**:
- Each file parse gets its own `Bump` arena
- Arena is reset after analysis results are extracted
- For parallel parsing with rayon, use `bumpalo-herd` to provide per-thread arenas

### ch-scanner

**Purpose**: Filesystem traversal and parallel file analysis with result caching.

**Key Components**:
- `walker.rs` - Directory traversal using `ignore` crate (respects `.gitignore`)
- `analyzer.rs` - Orchestrates parsing and aggregates results
- `cache.rs` - `FxHashMap` + `RwLock` concurrent cache for analysis results
- `stats.rs` - Statistics aggregation for migration progress

**Dependencies**: `ignore`, `rayon`, `parking_lot`

**Design Notes**:

```
Scanning Pipeline:
──────────────────
                    ┌─────────────┐
                    │   Walker    │
                    │  (ignore)   │
                    └──────┬──────┘
                           │ Stream of .ts/.tsx paths
                           ▼
                    ┌─────────────┐
                    │   Rayon     │
                    │  par_iter   │
                    └──────┬──────┘
                           │ Parallel parse jobs
                    ┌──────┴──────┐
                    ▼             ▼
              ┌──────────┐  ┌──────────┐
              │ Parser 1 │  │ Parser N │
              │ (arena)  │  │ (arena)  │
              └────┬─────┘  └────┬─────┘
                   │             │
                   └──────┬──────┘
                          │ Analysis results
                          ▼
                   ┌─────────────────────┐
                   │  FxHashMap + RwLock │
                   │       Cache         │
                   └─────────────────────┘
```

**Why `ignore` over `walkdir`**:
- Automatically respects `.gitignore` patterns
- Critical for large enterprise codebases with node_modules, dist, etc.
- Parallel iteration support built-in

### ch-watcher

**Purpose**: File change detection with debouncing and async event streaming.

**Key Components**:
- `watcher.rs` - Notify-based file watcher setup
- `events.rs` - Event types and channel management
- `debounce.rs` - Change batching to avoid excessive updates

**Dependencies**: `notify`, `notify-debouncer-mini`, `tokio`

**Design Notes**:

The `notify` crate is synchronous. To integrate with the async TUI event loop:

```rust
// Bridge sync notify to async tokio
let (tx, mut rx) = tokio::sync::mpsc::channel(100);

tokio::task::spawn_blocking(move || {
    let (notify_tx, notify_rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(notify_tx)?;
    watcher.watch(&path, RecursiveMode::Recursive)?;

    for event in notify_rx {
        // Send to async channel
        let _ = tx.blocking_send(event);
    }
});

// In async context
tokio::select! {
    Some(event) = rx.recv() => { /* handle file change */ }
    _ = shutdown_signal() => { /* cleanup */ }
}
```

**Debouncing Strategy**:
- Use `notify-debouncer-mini` with 100ms debounce window
- Batch rapid consecutive changes (common during save operations)
- Emit single consolidated event per file

### ch-tui

**Purpose**: Terminal user interface components using Ratatui.

**Key Components**:
- `app.rs` - Application state struct and lifecycle management
- `event.rs` - Event loop handling keyboard, mouse, and file events
- `ui.rs` - Main layout and rendering orchestration
- `components/` - Individual UI components

**Dependencies**: `ratatui`, `crossterm`, `tokio`, `color-eyre`

**Design Notes**:

**State Management Pattern** (Immutable Shared Reference):

```rust
// Recommended pattern from Ratatui docs
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render using immutable reference to app state
    }
}

// For stateful widgets (scroll position, selection)
impl StatefulWidget for FileList {
    type State = FileListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Widget logic separate from state
    }
}
```

**Component Architecture**:

```
┌─────────────────────────────────────────────────────────┐
│                      App (Root)                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │                  Header Bar                      │   │
│  │  Project path, scan status, keybindings hint    │   │
│  └─────────────────────────────────────────────────┘   │
│  ┌───────────────────────┬─────────────────────────┐   │
│  │                       │                         │   │
│  │     FileListView      │      StatsPanel         │   │
│  │                       │                         │   │
│  │  - File tree/list     │  - Total files          │   │
│  │  - Migration status   │  - Migrated count       │   │
│  │  - Selection state    │  - Remaining count      │   │
│  │                       │  - Progress bar         │   │
│  │                       │                         │   │
│  └───────────────────────┴─────────────────────────┘   │
│  ┌─────────────────────────────────────────────────┐   │
│  │                   StatusBar                      │   │
│  │  Current action, errors, last updated timestamp │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

**Event Loop Architecture**:

```rust
loop {
    // Render current state
    terminal.draw(|frame| ui::render(&app, frame))?;

    // Handle events with timeout for responsiveness
    tokio::select! {
        // Terminal events (keyboard, mouse, resize)
        Some(event) = terminal_events.recv() => {
            app.handle_terminal_event(event);
        }
        // File system events from watcher
        Some(event) = file_events.recv() => {
            app.handle_file_event(event);
        }
        // Periodic tick for animations/updates
        _ = tick_interval.tick() => {
            app.tick();
        }
    }

    if app.should_quit {
        break;
    }
}
```

### ch-cli

**Purpose**: Binary entry point with CLI argument parsing and application bootstrap.

**Key Components**:
- `main.rs` - Entry point, argument parsing, application orchestration

**Dependencies**: `clap`, `tokio`, `tracing-subscriber`, `color-eyre`

**Design Notes**:

```
CLI Structure:
──────────────
ch-migrate [OPTIONS] <COMMAND>

Commands:
  scan     Scan the codebase and show migration status
  watch    Start TUI with live file watching
  report   Generate migration report (JSON/CSV)

Options:
  -p, --path <PATH>     Path to WebApp.Desktop/src directory
  -v, --verbose         Enable verbose logging
  --no-color            Disable colored output
```

**Bootstrap Sequence**:
1. Parse CLI arguments with `clap`
2. Initialize `color-eyre` for error reporting
3. Set up `tracing-subscriber` with env filter
4. Load/validate configuration
5. Initialize scanner and perform initial scan
6. Start watcher and TUI event loop
7. Handle graceful shutdown on SIGINT/SIGTERM

## Data Flow

### Initial Scan

```
User starts ch-migrate
         │
         ▼
┌─────────────────┐
│ Parse CLI args  │
│ Load config     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Scanner walks   │
│ WebApp.Desktop  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│ For each .ts:   │────▶│ Parse imports   │
│ - Filter files  │     │ - tree-sitter   │
└─────────────────┘     └────────┬────────┘
                                 │
                                 ▼
                        ┌─────────────────┐
                        │ Classify:       │
                        │ - shared/       │
                        │ - shared_2023/  │
                        │ - both          │
                        │ - neither       │
                        └────────┬────────┘
                                 │
                                 ▼
                        ┌─────────────────┐
                        │ Cache results   │
                        │ Update stats    │
                        └────────┬────────┘
                                 │
                                 ▼
                        ┌─────────────────┐
                        │ Render TUI      │
                        └─────────────────┘
```

### File Change Event

```
Developer saves file
         │
         ▼
┌─────────────────┐
│ notify detects  │
│ file change     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Debounce        │
│ (batch changes) │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Send to async   │
│ channel         │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ TUI receives    │
│ file event      │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Incremental     │
│ re-parse file   │──── Uses tree.edit() for speed
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Update cache    │
│ Recalc stats    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Re-render TUI   │
└─────────────────┘
```

## Performance Considerations

### Parsing Performance

| Strategy | Benefit |
|----------|---------|
| Tree-sitter incremental parsing | O(edit size) not O(file size) on changes |
| Arena allocation (bumpalo) | Single allocation per file, no individual frees |
| Per-thread arenas (bumpalo-herd) | No lock contention during parallel parsing |
| FxHash over std HashMap | ~2x faster for string keys |

### Memory Efficiency

| Strategy | Benefit |
|----------|---------|
| Arena reset after analysis | Memory returned immediately, no fragmentation |
| SmallVec for import lists | Inline storage for typical case (<8 imports) |
| String interning | Deduplicate repeated model names |
| Streaming file walk | Constant memory regardless of file count |

### Concurrency Model

| Component | Strategy |
|-----------|----------|
| Initial scan | Rayon parallel iterator over files |
| File parsing | Per-thread parsers and arenas |
| Cache updates | RwLock-protected FxHashMap |
| Event loop | Tokio async with select! |
| File watching | spawn_blocking bridge to async |

## Error Handling

### Error Hierarchy

```
AppError (anyhow wrapper)
├── ConfigError
│   ├── InvalidPath
│   ├── MissingDirectory
│   └── InvalidOption
├── ScanError
│   ├── WalkError (from ignore)
│   ├── IoError (file read)
│   └── ParseError (tree-sitter)
├── WatchError
│   ├── NotifyError
│   └── ChannelClosed
└── TuiError
    ├── TerminalError
    └── RenderError
```

### Recovery Strategies

| Error Type | Recovery |
|------------|----------|
| Single file parse failure | Log warning, skip file, continue scan |
| Watch path doesn't exist | Error on startup, suggest valid path |
| Terminal resize fails | Attempt recovery, graceful degradation |
| Channel closed | Initiate graceful shutdown |

## Testing Strategy

### Unit Tests
- `ch-ts-parser`: Test import extraction with fixture TypeScript files
- `ch-core`: Test type serialization and configuration validation
- Use `insta` for snapshot testing of complex outputs

### Integration Tests
- `ch-scanner`: Test against mock directory structures
- `ch-watcher`: Test debouncing behavior with synthetic events

### Manual Testing
- TUI interaction testing with actual ClickHome repository
- Performance profiling with large codebases

## Future Considerations

1. **LSP Integration** - Consider using TypeScript LSP for more accurate analysis
2. **Auto-Migration** - Automated import rewriting (requires careful AST manipulation)
3. **Git Integration** - Show migration status per git branch
4. **CI Integration** - Export metrics for CI pipeline consumption
5. **Plugin System** - Support custom migration patterns beyond shared/shared_2023
