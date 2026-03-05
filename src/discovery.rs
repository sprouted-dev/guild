use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config::{ProjectConfig, WorkspaceConfig};
use crate::error::ConfigError;

/// Find the workspace root by searching for `guild.toml` with a `[workspace]` section,
/// starting from `start_dir` and walking up to parent directories.
pub fn find_workspace_root(start_dir: &Path) -> Result<PathBuf, ConfigError> {
    let mut current = start_dir.to_path_buf();
    loop {
        let candidate = current.join("guild.toml");
        if candidate.exists() {
            let content =
                std::fs::read_to_string(&candidate).map_err(|e| ConfigError::ReadFile {
                    path: candidate.clone(),
                    source: e,
                })?;
            if content.contains("[workspace]") {
                return Ok(current);
            }
        }
        if !current.pop() {
            return Err(ConfigError::WorkspaceNotFound {
                path: start_dir.to_path_buf(),
            });
        }
    }
}

/// Discover all project `guild.toml` files within the workspace.
///
/// Uses the workspace's project glob patterns to find project directories,
/// then looks for `guild.toml` in each matching directory.
pub fn discover_projects(workspace: &WorkspaceConfig) -> Result<Vec<ProjectConfig>, ConfigError> {
    let root = workspace.root();
    let mut projects = Vec::new();

    for pattern in workspace.project_patterns() {
        let full_pattern = root.join(pattern).to_string_lossy().to_string();
        let matches = glob::glob(&full_pattern).map_err(|e| ConfigError::ReadFile {
            path: PathBuf::from(&full_pattern),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()),
        })?;

        for entry in matches {
            let path = entry.map_err(|e| ConfigError::ReadFile {
                path: PathBuf::from(&full_pattern),
                source: std::io::Error::other(e.to_string()),
            })?;
            if path.is_dir() {
                let toml_path = path.join("guild.toml");
                if toml_path.exists() {
                    projects.push(ProjectConfig::from_file(&toml_path)?);
                }
            }
        }
    }

    // If no glob patterns matched, fall back to walking the directory tree
    if projects.is_empty() && workspace.project_patterns().is_empty() {
        for entry in WalkDir::new(root)
            .min_depth(1)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == "guild.toml" && entry.path() != root.join("guild.toml") {
                let content =
                    std::fs::read_to_string(entry.path()).map_err(|e| ConfigError::ReadFile {
                        path: entry.path().to_path_buf(),
                        source: e,
                    })?;
                if content.contains("[project]") {
                    projects.push(ProjectConfig::from_file(entry.path())?);
                }
            }
        }
    }

    Ok(projects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_find_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create workspace guild.toml
        fs::write(
            root.join("guild.toml"),
            "[workspace]\nname = \"test\"\nprojects = [\"apps/*\"]\n",
        )
        .unwrap();

        // Create nested dir
        let nested = root.join("apps").join("my-app");
        fs::create_dir_all(&nested).unwrap();

        let found = find_workspace_root(&nested).unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn test_find_workspace_root_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_workspace_root(dir.path()).is_err());
    }

    #[test]
    fn test_discover_projects() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create workspace config
        fs::write(
            root.join("guild.toml"),
            "[workspace]\nname = \"test\"\nprojects = [\"apps/*\"]\n",
        )
        .unwrap();

        // Create a project
        let app_dir = root.join("apps").join("my-app");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(
            app_dir.join("guild.toml"),
            "[project]\nname = \"my-app\"\n\n[targets.build]\ncommand = \"echo build\"\n",
        )
        .unwrap();

        let workspace = WorkspaceConfig::from_file(&root.join("guild.toml")).unwrap();
        let projects = discover_projects(&workspace).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name().as_str(), "my-app");
    }
}
