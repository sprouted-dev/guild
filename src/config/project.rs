use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::types::{DependsOn, ProjectName, TargetName};
use crate::error::ConfigError;

/// Raw deserialization target for a project `guild.toml`.
#[derive(Debug, Deserialize)]
struct ProjectToml {
    project: ProjectSection,
    #[serde(default)]
    targets: HashMap<TargetName, TargetSection>,
}

#[derive(Debug, Deserialize)]
struct ProjectSection {
    name: ProjectName,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    depends_on: Vec<ProjectName>,
}

#[derive(Debug, Deserialize)]
struct TargetSection {
    command: String,
    #[serde(default)]
    depends_on: Vec<DependsOn>,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
}

/// A validated project configuration parsed from a project's `guild.toml`.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    name: ProjectName,
    tags: Vec<String>,
    depends_on: Vec<ProjectName>,
    targets: HashMap<TargetName, TargetConfig>,
    root: PathBuf,
}

/// A validated target configuration.
#[derive(Debug, Clone)]
pub struct TargetConfig {
    command: String,
    depends_on: Vec<DependsOn>,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

impl ProjectConfig {
    /// Parse a project configuration from the given `guild.toml` path.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadFile {
            path: path.to_path_buf(),
            source: e,
        })?;
        let root = path
            .parent()
            .expect("guild.toml must have a parent directory")
            .to_path_buf();
        Self::from_str(&content, root)
    }

    /// Parse a project configuration from a TOML string (for testing).
    pub fn from_str(content: &str, root: PathBuf) -> Result<Self, ConfigError> {
        let raw: ProjectToml = toml::from_str(content).map_err(|e| ConfigError::ParseToml {
            path: root.join("guild.toml"),
            source: e,
        })?;
        let targets = raw
            .targets
            .into_iter()
            .map(|(name, section)| {
                let config = TargetConfig {
                    command: section.command,
                    depends_on: section.depends_on,
                    inputs: section.inputs,
                    outputs: section.outputs,
                };
                (name, config)
            })
            .collect();
        Ok(Self {
            name: raw.project.name,
            tags: raw.project.tags,
            depends_on: raw.project.depends_on,
            targets,
            root,
        })
    }

    pub fn name(&self) -> &ProjectName {
        &self.name
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn depends_on(&self) -> &[ProjectName] {
        &self.depends_on
    }

    pub fn targets(&self) -> &HashMap<TargetName, TargetConfig> {
        &self.targets
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl TargetConfig {
    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn depends_on(&self) -> &[DependsOn] {
        &self.depends_on
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_config() {
        let toml = r#"
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
"#;
        let config = ProjectConfig::from_str(toml, PathBuf::from("/tmp/my-app")).unwrap();
        assert_eq!(config.name().as_str(), "my-app");
        assert_eq!(config.tags(), &["app", "typescript"]);
        assert_eq!(config.depends_on().len(), 1);
        assert_eq!(config.depends_on()[0].as_str(), "shared-lib");
        assert_eq!(config.targets().len(), 3);

        let build = &config.targets()[&"build".parse::<TargetName>().unwrap()];
        assert_eq!(build.command(), "npm run build");
        assert_eq!(build.depends_on().len(), 1);
        assert!(build.depends_on()[0].is_upstream());
    }

    #[test]
    fn test_parse_minimal_project() {
        let toml = r#"
[project]
name = "minimal"
"#;
        let config = ProjectConfig::from_str(toml, PathBuf::from("/tmp/minimal")).unwrap();
        assert_eq!(config.name().as_str(), "minimal");
        assert!(config.tags().is_empty());
        assert!(config.depends_on().is_empty());
        assert!(config.targets().is_empty());
    }

    #[test]
    fn test_parse_project_invalid_name() {
        let toml = r#"
[project]
name = "My App"
"#;
        assert!(ProjectConfig::from_str(toml, PathBuf::from("/tmp")).is_err());
    }

    #[test]
    fn test_target_with_inputs_outputs() {
        let toml = r#"
[project]
name = "my-app"

[targets.build]
command = "cargo build"
inputs = ["src/**/*.rs", "Cargo.toml"]
outputs = ["target/release/my-app"]
"#;
        let config = ProjectConfig::from_str(toml, PathBuf::from("/tmp/my-app")).unwrap();
        let build = &config.targets()[&"build".parse::<TargetName>().unwrap()];
        assert_eq!(build.inputs(), &["src/**/*.rs", "Cargo.toml"]);
        assert_eq!(build.outputs(), &["target/release/my-app"]);
    }
}
