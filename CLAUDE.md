# Guild

Rust-native polyglot monorepo orchestrator. Task dependency graphs, parallel execution, affected detection, and caching — without the Node.js ecosystem.

## Architecture

```
src/
├── main.rs          # Entry point — thin, delegates to lib
├── lib.rs           # Public API re-exports
├── cli.rs           # Clap CLI definitions (all subcommands)
├── error.rs         # Error types (ParseError, ConfigError)
├── config/
│   ├── mod.rs       # Re-exports
│   ├── workspace.rs # Root guild.toml [workspace] parsing
│   ├── project.rs   # Per-project guild.toml [project] + [targets] parsing
│   └── types.rs     # Strong types: ProjectName, TargetName, DependsOn
├── discovery.rs     # Walk filesystem for guild.toml files
├── graph/
│   ├── mod.rs       # Re-exports
│   └── project.rs   # Project dependency graph (DAG construction + validation)
└── output.rs        # Terminal output formatting (colored)
```

## Core Config Format

Guild uses `guild.toml` files — one at workspace root, one per project:

**Root `guild.toml`:**
```toml
[workspace]
name = "my-monorepo"
projects = ["apps/*", "libs/*"]  # glob patterns
```

**Project `guild.toml`:**
```toml
[project]
name = "my-app"
tags = ["app", "typescript"]

[targets.build]
command = "npm run build"
depends_on = ["^build"]  # ^ means dependency projects' build target

[targets.test]
command = "npm test"
depends_on = ["build"]   # local build target first

[targets.lint]
command = "npm run lint"
```

## Coding Conventions

Follow Sprouted Rust conventions (see `~/sprouted/projects/terra-platform/RUST_CONVENTIONS.md`):

- **Edition 2024**, toolchain pinned via `rust-toolchain.toml`
- **Private modules, public re-exports** via `lib.rs`
- **Strong types** — `ProjectName`, `TargetName`, `DependsOn` are newtypes with validation at construction
- **`FromStr`/`Display` roundtrip** on all domain types, verified with proptest
- **`thiserror`** for all error types, **`anyhow`** only in `main.rs`
- **No panics, no `.unwrap()`** in library code
- **No env var reading** in library code
- **Tokio** async runtime
- **`cargo fmt`** + **`cargo clippy -- -D warnings`** enforced in CI and pre-commit

## Development

```bash
just setup    # Configure git hooks
just check    # Format + lint + test
just build    # Release build
```

## Testing

- Unit tests in `#[cfg(test)] mod tests` within source files
- Property tests in `tests/property_tests.rs` for type roundtrips
- Integration tests in `tests/` for discovery and config parsing
