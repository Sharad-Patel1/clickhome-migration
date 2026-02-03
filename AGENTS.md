# AGENTS.md

Instructions for AI coding assistants working on this codebase.

## Critical Rules

**ALWAYS do before completing any task:**
- Run `cargo check --workspace` to verify type safety
- Run `cargo clippy --workspace` to verify lint compliance
- Fix any errors or warnings before considering work complete
- **Fix the root cause of linter issues** - do not just disable lints with `#[allow(...)]` or delete code to silence warnings

**NEVER do unless explicitly requested by the user:**
- Do NOT run `cargo build` - only run check/clippy
- Do NOT run `cargo test` - only run when asked
- Do NOT run the binary (`cargo run`) - only run when asked

## Project Overview

This is `ch-migration`, a Rust TUI application for migrating TypeScript models from `shared` to `shared_2023` in the ClickHome enterprise codebase. It uses tree-sitter for parsing, Ratatui for the terminal UI, and tokio for async operations.

## Code Style

### Rust Edition & Toolchain

- **Edition**: Rust 2024
- **MSRV**: 1.85
- **Formatter**: `cargo fmt` (see `rustfmt.toml`)
- **Linter**: `cargo clippy` with pedantic lints enabled

### Paradigm Preferences

**DO:**
- Prefer composition over inheritance
- Write pure functions where possible
- Use functional/procedural style with iterators and combinators
- Keep side effects at the edges of the system
- Return `Result<T, E>` for fallible operations
- **Always prefer zero-cost abstractions** - no runtime overhead for abstractions
- Use generics and monomorphization over dynamic dispatch (trait objects)
- Prefer `impl Trait` over `Box<dyn Trait>` when possible
- Use `#[inline]` judiciously for small, hot functions

**DON'T:**
- Don't use classic OOP patterns (deep inheritance, abstract factories)
- Don't use `unwrap()` or `expect()` - they are denied by clippy
- Don't use `panic!()`, `todo!()`, or `unimplemented!()`
- Don't use `std::thread::spawn` - use rayon or tokio instead
- Don't use `println!`/`eprintln!` except in xtask

### Type Preferences

**Use these instead of std types:**

| Instead of | Use | Reason |
|------------|-----|--------|
| `std::collections::HashMap` | `rustc_hash::FxHashMap` | Faster for string keys |
| `std::collections::HashSet` | `rustc_hash::FxHashSet` | Faster for string keys |
| `std::sync::Mutex` | `parking_lot::Mutex` | Better performance |
| `std::sync::RwLock` | `parking_lot::RwLock` | Better performance |
| `std::path::Path` | `camino::Utf8Path` | Guaranteed UTF-8 |
| `Vec<T>` (small) | `smallvec::SmallVec<[T; N]>` | Stack allocation for small vecs |

### Error Handling

```rust
// Define errors with thiserror in each crate
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("failed to walk directory: {0}")]
    Walk(#[from] ignore::Error),
    
    #[error("failed to read file {path}: {source}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// Use anyhow at application boundaries (ch-cli)
fn main() -> anyhow::Result<()> {
    // ...
}
```

### Function Signatures

```rust
// Prefer borrowing over ownership when possible
fn analyze_file(path: &Utf8Path, contents: &str) -> Result<FileAnalysis, ParseError>

// Use impl Trait for return types when appropriate
fn iter_imports(&self) -> impl Iterator<Item = &ImportInfo>

// Use generics with trait bounds sparingly
fn process<P: AsRef<Utf8Path>>(path: P) -> Result<()>
```

## Workspace Structure

```
ch-migration/
├── crates/
│   ├── ch-core/        # Shared types, errors, config (no async)
│   ├── ch-ts-parser/   # Tree-sitter TypeScript parsing
│   ├── ch-scanner/     # Parallel file discovery (rayon)
│   ├── ch-watcher/     # File watching (notify + tokio)
│   ├── ch-tui/         # Ratatui UI components
│   └── ch-cli/         # Binary entry point
├── xtask/              # Build automation
└── docs/               # Documentation
```

### Crate Dependencies

```
ch-cli → ch-tui → ch-scanner → ch-ts-parser → ch-core
                → ch-watcher ───────────────→ ch-core
```

**Rules:**
- `ch-core` has no internal dependencies
- Lower crates don't depend on higher crates
- Only `ch-cli` and `xtask` are binaries

## Patterns to Follow

### Ratatui Components

Use the **immutable shared reference** pattern for widgets:

```rust
pub struct FileList {
    files: Vec<FileInfo>,
    filter: Option<String>,
}

impl Widget for &FileList {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render logic here
    }
}
```

For stateful widgets (scroll, selection), use `StatefulWidget`:

```rust
pub struct FileListState {
    selected: Option<usize>,
    scroll_offset: usize,
}

impl StatefulWidget for &FileList {
    type State = FileListState;
    
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Render with mutable state access
    }
}
```

### Tree-sitter Parsing

```rust
use tree_sitter::{Parser, Language};

fn create_parser() -> Result<Parser, ParseError> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
    Ok(parser)
}

// For incremental parsing on file changes
fn reparse(parser: &mut Parser, old_tree: &Tree, new_source: &str, edit: InputEdit) -> Tree {
    let mut tree = old_tree.clone();
    tree.edit(&edit);
    parser.parse(new_source, Some(&tree)).expect("parse failed")
}
```

### Async + Sync Bridge (for notify)

```rust
use tokio::sync::mpsc;
use notify::{Watcher, RecursiveMode};

async fn watch_files(path: &Utf8Path) -> mpsc::Receiver<notify::Event> {
    let (tx, rx) = mpsc::channel(100);
    
    tokio::task::spawn_blocking(move || {
        let (notify_tx, notify_rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(notify_tx).unwrap();
        watcher.watch(path.as_std_path(), RecursiveMode::Recursive).unwrap();
        
        for event in notify_rx {
            if tx.blocking_send(event).is_err() {
                break;
            }
        }
    });
    
    rx
}
```

### Parallel Processing with Rayon

```rust
use rayon::prelude::*;

fn scan_files(paths: &[Utf8PathBuf]) -> Vec<Result<FileAnalysis, ScanError>> {
    paths
        .par_iter()
        .map(|path| analyze_file(path))
        .collect()
}
```

### Arena Allocation

```rust
use bumpalo::Bump;

fn parse_with_arena(source: &str) -> FileAnalysis {
    let arena = Bump::new();
    
    // Allocate strings in arena
    let interned = arena.alloc_str(source);
    
    // Parse and extract results
    let result = do_parsing(&arena, interned);
    
    // Arena is dropped here, memory freed
    result
}
```

### Zero-Cost Abstractions

Always prefer abstractions that compile away to the same code you'd write by hand:

```rust
// GOOD: Zero-cost - monomorphized at compile time
fn process<I: Iterator<Item = FileInfo>>(iter: I) -> Vec<FileInfo> {
    iter.filter(|f| f.is_typescript())
        .collect()
}

// GOOD: Zero-cost - impl Trait is static dispatch
fn get_files(&self) -> impl Iterator<Item = &FileInfo> {
    self.files.iter().filter(|f| f.needs_migration())
}

// AVOID: Runtime cost - dynamic dispatch, heap allocation
fn process_dynamic(iter: Box<dyn Iterator<Item = FileInfo>>) -> Vec<FileInfo> {
    iter.filter(|f| f.is_typescript())
        .collect()
}

// GOOD: Newtype pattern - zero runtime overhead
pub struct FileId(u32);

// GOOD: Inline for small, frequently called functions
#[inline]
fn is_typescript_ext(ext: &str) -> bool {
    matches!(ext, "ts" | "tsx")
}
```

**When dynamic dispatch IS acceptable:**
- Plugin systems or user-extensible code
- Heterogeneous collections where variants aren't known at compile time
- When compile time or binary size is a bigger concern than runtime

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_import_extraction() {
        let source = r#"import { Foo } from '../shared/models/foo';"#;
        let imports = extract_imports(source).unwrap();
        
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source_path, "../shared/models/foo");
    }
}
```

### Snapshot Tests with Insta

```rust
#[test]
fn test_analysis_output() {
    let result = analyze_file("fixtures/sample.ts").unwrap();
    insta::assert_json_snapshot!(result);
}
```

## Documentation

### Module Documentation

```rust
//! Brief description of the module.
//!
//! More detailed explanation of what this module provides,
//! when to use it, and any important considerations.
//!
//! # Examples
//!
//! ```
//! use ch_scanner::Scanner;
//!
//! let scanner = Scanner::new("/path/to/project");
//! let results = scanner.scan()?;
//! ```
```

### Function Documentation

```rust
/// Analyzes a TypeScript file for model imports.
///
/// Parses the file using tree-sitter and extracts all import statements
/// that reference the `shared` or `shared_2023` directories.
///
/// # Arguments
///
/// * `path` - Path to the TypeScript file
/// * `contents` - File contents as a string
///
/// # Returns
///
/// Returns `FileAnalysis` containing all detected imports and their classification.
///
/// # Errors
///
/// Returns `ParseError` if the file cannot be parsed as valid TypeScript.
pub fn analyze_file(path: &Utf8Path, contents: &str) -> Result<FileAnalysis, ParseError>
```

## Common Tasks

### Adding a New Crate

1. Create with cargo: `cargo new --lib crates/ch-newcrate`
2. Update `Cargo.toml` to inherit workspace settings:
   ```toml
   [package]
   name = "ch-newcrate"
   version.workspace = true
   edition.workspace = true
   # ... other workspace inherits
   
   [lints]
   workspace = true
   ```
3. Add dependencies from `[workspace.dependencies]`
4. Update `docs/ARCHITECTURE.md` with the new crate

### Adding a Dependency

1. Add to `[workspace.dependencies]` in root `Cargo.toml` with version
2. Add to crate's `Cargo.toml` with `.workspace = true`:
   ```toml
   [dependencies]
   new-dep.workspace = true
   ```

### Running Commands

```bash
cargo c          # cargo check
cargo cl         # cargo clippy
cargo cla        # clippy --all-targets --all-features
cargo cli -- -h  # run ch-migrate binary with args
cargo xt check   # run xtask check command
```

## Things to Avoid

1. **Don't create new files unnecessarily** - Edit existing files when possible
2. **Don't add dependencies without workspace entry** - All deps go through workspace
3. **Don't use blocking I/O in async contexts** - Use `spawn_blocking` or async alternatives
4. **Don't suppress lints without fixing the root cause** - Always fix the underlying issue:
   - If clippy warns about unused code, determine if the code should be used or removed
   - If clippy warns about complexity, refactor to reduce complexity
   - If clippy warns about a pattern, use the suggested better pattern
   - `#[allow(...)]` is only acceptable with a comment explaining why the lint doesn't apply
5. **Don't hardcode paths** - Use `camino::Utf8Path` and accept paths as parameters
6. **Don't mix sync and async carelessly** - Be explicit about boundaries
7. **Don't use `String` when `&str` suffices** - Prefer borrowing
8. **Don't allocate in hot loops** - Use arena allocation or pre-allocation
9. **Don't delete code just to silence warnings** - Understand why the warning exists first

## Architecture Reference

See `docs/ARCHITECTURE.md` for:
- Detailed crate responsibilities
- Data flow diagrams
- Performance considerations
- Error handling hierarchy
