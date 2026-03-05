use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ConfigError;

/// Raw deserialization target for the root `guild.toml`.
#[derive(Debug, Deserialize)]
struct WorkspaceToml {
    workspace: WorkspaceSection,
}

#[derive(Debug, Deserialize)]
struct WorkspaceSection {
    name: String,
    projects: Vec<String>,
}

/// A validated workspace configuration parsed from the root `guild.toml`.
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    /// Workspace name.
    name: String,
    /// Glob patterns for discovering project directories.
    project_patterns: Vec<String>,
    /// Absolute path to the workspace root directory.
    root: PathBuf,
}

impl WorkspaceConfig {
    /// Parse a workspace configuration from the given `guild.toml` path.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;
        let raw: WorkspaceToml = toml::from_str(&content).map_err(|e| ConfigError::ParseToml {
            path: path.to_path_buf(),
            source: e,
        })?;
        let root = path
            .parent()
            .expect("guild.toml must have a parent directory")
            .to_path_buf();
        Ok(Self {
            name: raw.workspace.name,
            project_patterns: raw.workspace.projects,
            root,
        })
    }

    /// Parse a workspace configuration from a TOML string (for testing).
    pub fn from_str(content: &str, root: PathBuf) -> Result<Self, ConfigError> {
        let raw: WorkspaceToml = toml::from_str(content).map_err(|e| ConfigError::ParseToml {
            path: root.join("guild.toml"),
            source: e,
        })?;
        Ok(Self {
            name: raw.workspace.name,
            project_patterns: raw.workspace.projects,
            root,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn project_patterns(&self) -> &[String] {
        &self.project_patterns
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workspace_config() {
        let toml = r#"
[workspace]
name = "my-monorepo"
projects = ["apps/*", "libs/*"]
"#;
        let config = WorkspaceConfig::from_str(toml, PathBuf::from("/tmp")).unwrap();
        assert_eq!(config.name(), "my-monorepo");
        assert_eq!(config.project_patterns(), &["apps/*", "libs/*"]);
        assert_eq!(config.root(), Path::new("/tmp"));
    }

    #[test]
    fn test_parse_workspace_missing_name() {
        let toml = r#"
[workspace]
projects = ["apps/*"]
"#;
        assert!(WorkspaceConfig::from_str(toml, PathBuf::from("/tmp")).is_err());
    }

    #[test]
    fn test_parse_workspace_missing_projects() {
        let toml = r#"
[workspace]
name = "test"
"#;
        assert!(WorkspaceConfig::from_str(toml, PathBuf::from("/tmp")).is_err());
    }
}
