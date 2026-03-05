use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "guild",
    version,
    about = "Rust-native polyglot monorepo orchestrator",
    long_about = "Guild provides task dependency graphs, parallel execution, affected detection, \
                  and caching for polyglot monorepos — without the Node.js ecosystem."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start all dev targets
    Dev,

    /// Build everything
    Build,

    /// Test everything
    Test,

    /// Lint everything
    Lint,

    /// Run an arbitrary target
    Run {
        /// Target name to run
        target: String,
        /// Optional project to scope the target to
        project: Option<String>,
    },

    /// Run a target on affected projects only
    Affected {
        /// Target name to run on affected projects
        target: String,
    },

    /// List all discovered projects
    List,

    /// Show the project dependency graph
    Graph,

    /// Cache management
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },

    /// Scaffold guild.toml from existing manifests
    Init,
}

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Show cache statistics
    Status,
    /// Clear the cache
    Clean,
}
