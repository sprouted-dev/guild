default:
    @just --list

# Initial setup: configure git hooks
setup:
    git config core.hooksPath .githooks

# Format all code
fmt:
    cargo fmt

# Check formatting
fmt-check:
    cargo fmt --check

# Run clippy
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run tests
test:
    cargo test

# Build in release mode
build:
    cargo build --release

# Run all checks (format, lint, test)
check: fmt-check lint test

# Create a release tag and push it (usage: just release 0.1.0)
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo_version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
    if [ "$cargo_version" != "{{version}}" ]; then
        echo "Error: Cargo.toml version ($cargo_version) does not match {{version}}"
        exit 1
    fi
    if [ -n "$(git status --porcelain)" ]; then
        echo "Error: working directory is not clean"
        exit 1
    fi
    branch=$(git branch --show-current)
    if [ "$branch" != "main" ]; then
        echo "Error: not on main branch (on $branch)"
        exit 1
    fi
    git tag -a "v{{version}}" -m "v{{version}}"
    git push origin "v{{version}}"
    echo "Tagged and pushed v{{version}}"
