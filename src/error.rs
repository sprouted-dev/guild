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
