# Guild

Rust-native polyglot monorepo orchestrator. Task dependency graphs, parallel execution, affected detection, and caching — without the Node.js ecosystem.

## Installation

### Homebrew (macOS/Linux)

```bash
brew install sprouted-dev/tap/guild
```

### Cargo

```bash
cargo install guild-cli
```

### From source

```bash
git clone https://github.com/sprouted-dev/guild.git
cd guild
cargo install --path .
```

## Quick Start

### 1. Create a workspace `guild.toml` at your monorepo root

```toml
[workspace]
name = "my-monorepo"
projects = ["apps/*", "libs/*"]
```

### 2. Add a `guild.toml` to each project

```toml
[project]
name = "my-app"
tags = ["app", "typescript"]
depends_on = ["shared-lib"]

[targets.build]
command = "npm run build"
depends_on = ["^build"]

[targets.test]
command = "npm test"
depends_on = ["build"]

[targets.lint]
command = "npm run lint"
```

### 3. Run targets

```bash
guild build         # Build everything (respecting dependency order)
guild test          # Test everything
guild lint          # Lint everything
guild run build     # Run arbitrary target
guild list          # List all discovered projects
guild graph         # Show dependency graph
```

## `guild.toml` Format

### Workspace (root)

```toml
[workspace]
name = "my-monorepo"
projects = ["apps/*", "libs/*"]   # Glob patterns for project directories
```

### Project

```toml
[project]
name = "my-project"               # Unique project identifier
tags = ["lib", "rust"]             # Optional tags for filtering
depends_on = ["other-project"]     # Project-level dependencies

[targets.build]
command = "cargo build"            # Shell command to execute
depends_on = ["^build"]            # ^ = upstream projects' target
inputs = ["src/**/*.rs"]           # Files that affect cache validity
outputs = ["target/release/bin"]   # Files produced by the target

[targets.test]
command = "cargo test"
depends_on = ["build"]             # Local target dependency (no ^)
```

### Dependency Syntax

- `"build"` — depends on the `build` target **in the same project**
- `"^build"` — depends on the `build` target **in all upstream dependency projects**

## CLI Commands

```
guild                          Show help
guild dev                      Start all dev targets
guild build                    Build everything
guild test                     Test everything
guild lint                     Lint everything
guild run <target> [project]   Run arbitrary target
guild affected <target>        Run target on affected projects only
guild list                     List all discovered projects
guild graph                    Show dependency graph
guild cache status             Show cache statistics
guild cache clean              Clear the cache
guild init                     Scaffold guild.toml from existing manifests
```

## Development

```bash
just setup    # Configure git hooks
just check    # Format + lint + test
just build    # Release build
```

## License

MIT
