use std::path::PathBuf;
use thiserror::Error;

/// Errors from parsing configuration values and domain types.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("invalid project name '{value}': {reason}")]
    ProjectName { value: String, reason: String },

    #[error("invalid target name '{value}': {reason}")]
    TargetName { value: String, reason: String },

    #[error("invalid depends_on '{value}': {reason}")]
    DependsOn { value: String, reason: String },
}

/// Errors from loading and processing configuration files.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error("failed to read '{path}': {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse '{path}': {source}")]
    ParseToml {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("no guild.toml found in '{path}' or any parent directory")]
    WorkspaceNotFound { path: PathBuf },
}

/// Errors from building and validating the project dependency graph.
#[derive(Debug, Error)]
pub enum GraphError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("cycle detected in project dependencies: {cycle}")]
    CycleDetected { cycle: String },

    #[error("unknown project '{name}' referenced in depends_on of '{referenced_by}'")]
    UnknownProject { name: String, referenced_by: String },
}

/// Errors from building and validating the task dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TaskGraphError {
    #[error("cycle detected in task dependencies: {project}:{target}")]
    CycleDetected { project: String, target: String },

    #[error(
        "unknown target '{target}' referenced in depends_on of '{project}:{referencing_target}'"
    )]
    UnknownTarget {
        target: String,
        project: String,
        referencing_target: String,
    },

    #[error("unknown project '{project}' referenced in task graph")]
    UnknownProject { project: String },

    #[error("task '{project}:{target}' not found in graph")]
    TaskNotFound { project: String, target: String },
}

/// Errors from running tasks.
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("task graph error: {0}")]
    TaskGraph(#[from] TaskGraphError),

    #[error("failed to spawn command: {message}")]
    SpawnFailed { message: String },

    #[error("task '{project}:{target}' has no command configured")]
    NoCommand { project: String, target: String },
}

/// Errors from initializing a workspace with `guild init`.
#[derive(Debug, Error)]
pub enum InitError {
    #[error("failed to read '{path}': {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to write '{path}': {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse JSON '{path}': {source}")]
    ParseJson {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("failed to parse TOML '{path}': {source}")]
    ParseToml {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("failed to walk directory '{path}': {source}")]
    WalkDir {
        path: PathBuf,
        source: walkdir::Error,
    },

    #[error("invalid path: {path}")]
    InvalidPath { path: PathBuf },

    #[error("I/O error: {source}")]
    Io { source: std::io::Error },
}
