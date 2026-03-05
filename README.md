# Guild

Rust-native polyglot monorepo orchestrator. Task dependency graphs, parallel execution, affected detection, and caching — without the Node.js ecosystem.

Guild treats every language as a first-class citizen. Whether your monorepo mixes Rust, TypeScript, Go, Python, or anything else — if it has shell commands, Guild can orchestrate it.

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
projects = ["apps/*", "libs/*", "services/*"]
```

### 2. Add a `guild.toml` to each project

Guild works with any language — just point targets at shell commands:

**Rust project:**
```toml
[project]
name = "core-lib"
tags = ["lib", "rust"]

[targets.build]
command = "cargo build"
depends_on = ["^build"]
inputs = ["src/**/*.rs", "Cargo.toml"]

[targets.test]
command = "cargo test"
depends_on = ["build"]

[targets.lint]
command = "cargo clippy -- -D warnings"
```

**TypeScript project:**
```toml
[project]
name = "web-app"
tags = ["app", "typescript"]
depends_on = ["core-lib", "shared-utils"]

[targets.build]
command = "pnpm build"
depends_on = ["^build"]
inputs = ["src/**/*.ts", "package.json"]

[targets.test]
command = "pnpm test"
depends_on = ["build"]

[targets.dev]
command = "pnpm dev"

[targets.lint]
command = "pnpm lint"
```

**Go service:**
```toml
[project]
name = "api-gateway"
tags = ["service", "go"]
depends_on = ["core-lib"]

[targets.build]
command = "go build ./..."
depends_on = ["^build"]
inputs = ["**/*.go", "go.mod"]

[targets.test]
command = "go test ./..."
depends_on = ["build"]

[targets.lint]
command = "golangci-lint run"
```

### 3. Run targets

```bash
guild build                  # Build everything (respecting dependency order)
guild test                   # Test everything
guild lint                   # Lint everything
guild run build web-app      # Build a specific project (+ its dependencies)
guild affected test          # Test only what changed since main
guild list                   # List all discovered projects
guild graph                  # Show dependency graph
```

## Or bootstrap from existing manifests

Already have `Cargo.toml`, `package.json`, `go.mod`, or `pyproject.toml` files? Guild can scaffold config for you:

```bash
guild init                   # Detect projects and generate guild.toml files
guild init --yes             # Skip confirmation prompts
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
