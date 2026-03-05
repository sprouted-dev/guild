//! Manifest detection and guild.toml generation for `guild init`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, BufRead, Write as IoWrite};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use crate::error::InitError;
use crate::output::{print_success, print_warning};

/// Represents a detected project manifest.
#[derive(Debug, Clone)]
pub struct DetectedProject {
    /// Project name extracted from the manifest.
    pub name: String,
    /// Relative path from workspace root to the project directory.
    pub relative_path: PathBuf,
    /// Absolute path to the project directory.
    pub absolute_path: PathBuf,
    /// The type of project (Node, Rust, Go, Python).
    #[allow(dead_code)]
    pub project_type: ProjectType,
    /// Detected targets for this project.
    pub targets: BTreeMap<String, DetectedTarget>,
    /// Tags for this project based on its type and location.
    pub tags: Vec<String>,
}

/// The type of project detected from its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    Node,
    Rust,
    Go,
    Python,
}

impl ProjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectType::Node => "node",
            ProjectType::Rust => "rust",
            ProjectType::Go => "go",
            ProjectType::Python => "python",
        }
    }
}

/// A detected target for a project.
#[derive(Debug, Clone)]
pub struct DetectedTarget {
    pub command: String,
    pub depends_on: Vec<String>,
}

/// Result of running `guild init`.
#[derive(Debug)]
pub struct InitResult {
    /// Files that were written.
    pub written: Vec<PathBuf>,
    /// Files that were skipped because they already exist.
    pub skipped: Vec<PathBuf>,
}

/// Scan a directory tree for project manifests.
pub fn detect_projects(root: &Path) -> Result<Vec<DetectedProject>, InitError> {
    let mut projects = Vec::new();
    let mut seen_dirs = HashSet::new();

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip common non-project directories
            !matches!(
                name.as_ref(),
                "node_modules" | "target" | ".git" | "vendor" | "__pycache__" | ".venv" | "venv"
            )
        })
    {
        let entry = entry.map_err(|e| InitError::WalkDir {
            path: root.to_path_buf(),
            source: e,
        })?;

        if !entry.file_type().is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        let dir = entry
            .path()
            .parent()
            .ok_or_else(|| InitError::InvalidPath {
                path: entry.path().to_path_buf(),
            })?;

        // Skip if we've already found a project in this directory
        if seen_dirs.contains(dir) {
            continue;
        }

        let project = match file_name.as_ref() {
            "package.json" => detect_node_project(root, dir)?,
            "Cargo.toml" => detect_rust_project(root, dir)?,
            "go.mod" => detect_go_project(root, dir)?,
            "pyproject.toml" => detect_python_project(root, dir)?,
            _ => None,
        };

        if let Some(p) = project {
            seen_dirs.insert(dir.to_path_buf());
            projects.push(p);
        }
    }

    // Sort by relative path for consistent output
    projects.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok(projects)
}

/// Detect a Node.js project from package.json.
fn detect_node_project(root: &Path, dir: &Path) -> Result<Option<DetectedProject>, InitError> {
    let manifest_path = dir.join("package.json");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| InitError::ReadFile {
        path: manifest_path.clone(),
        source: e,
    })?;

    #[derive(Deserialize)]
    struct PackageJson {
        name: Option<String>,
        scripts: Option<HashMap<String, String>>,
    }

    let pkg: PackageJson = serde_json::from_str(&content).map_err(|e| InitError::ParseJson {
        path: manifest_path,
        source: e,
    })?;

    let name = match pkg.name {
        Some(n) => sanitize_project_name(&n),
        None => dir
            .file_name()
            .map(|s| sanitize_project_name(&s.to_string_lossy()))
            .unwrap_or_else(|| "unnamed".to_string()),
    };

    let mut targets = BTreeMap::new();

    // Extract common targets from scripts
    if let Some(scripts) = pkg.scripts {
        // Map script names to guild target names
        let script_mappings = [
            ("build", "build"),
            ("test", "test"),
            ("lint", "lint"),
            ("dev", "dev"),
            ("start", "start"),
            ("typecheck", "typecheck"),
            ("type-check", "typecheck"),
        ];

        for (script_name, target_name) in &script_mappings {
            if scripts.contains_key(*script_name) {
                let mut depends_on = Vec::new();
                // build target depends on upstream builds
                if *target_name == "build" {
                    depends_on.push("^build".to_string());
                }
                // test depends on local build
                if *target_name == "test" && targets.contains_key("build") {
                    depends_on.push("build".to_string());
                }
                targets.insert(
                    (*target_name).to_string(),
                    DetectedTarget {
                        command: format!("npm run {script_name}"),
                        depends_on,
                    },
                );
            }
        }
    }

    let relative_path = dir
        .strip_prefix(root)
        .map_err(|_| InitError::InvalidPath {
            path: dir.to_path_buf(),
        })?
        .to_path_buf();

    // Skip root package.json if it's at the workspace root
    if relative_path.as_os_str().is_empty() {
        return Ok(None);
    }

    let mut tags = vec![ProjectType::Node.as_str().to_string()];
    infer_tags_from_path(&relative_path, &mut tags);

    Ok(Some(DetectedProject {
        name,
        relative_path,
        absolute_path: dir.to_path_buf(),
        project_type: ProjectType::Node,
        targets,
        tags,
    }))
}

/// Detect a Rust project from Cargo.toml.
fn detect_rust_project(root: &Path, dir: &Path) -> Result<Option<DetectedProject>, InitError> {
    let manifest_path = dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| InitError::ReadFile {
        path: manifest_path.clone(),
        source: e,
    })?;

    #[derive(Deserialize)]
    struct CargoToml {
        package: Option<CargoPackage>,
    }

    #[derive(Deserialize)]
    struct CargoPackage {
        name: String,
    }

    let cargo: CargoToml = toml::from_str(&content).map_err(|e| InitError::ParseToml {
        path: manifest_path,
        source: e,
    })?;

    // Skip workspace-only Cargo.toml files (no package section)
    let name = match cargo.package {
        Some(pkg) => sanitize_project_name(&pkg.name),
        None => return Ok(None),
    };

    let relative_path = dir
        .strip_prefix(root)
        .map_err(|_| InitError::InvalidPath {
            path: dir.to_path_buf(),
        })?
        .to_path_buf();

    // Skip root Cargo.toml if it's at the workspace root
    if relative_path.as_os_str().is_empty() {
        return Ok(None);
    }

    // Standard Rust targets
    let mut targets = BTreeMap::new();
    targets.insert(
        "build".to_string(),
        DetectedTarget {
            command: "cargo build".to_string(),
            depends_on: vec!["^build".to_string()],
        },
    );
    targets.insert(
        "test".to_string(),
        DetectedTarget {
            command: "cargo test".to_string(),
            depends_on: vec!["build".to_string()],
        },
    );
    targets.insert(
        "lint".to_string(),
        DetectedTarget {
            command: "cargo clippy -- -D warnings".to_string(),
            depends_on: vec![],
        },
    );

    let mut tags = vec![ProjectType::Rust.as_str().to_string()];
    infer_tags_from_path(&relative_path, &mut tags);

    Ok(Some(DetectedProject {
        name,
        relative_path,
        absolute_path: dir.to_path_buf(),
        project_type: ProjectType::Rust,
        targets,
        tags,
    }))
}

/// Detect a Go project from go.mod.
fn detect_go_project(root: &Path, dir: &Path) -> Result<Option<DetectedProject>, InitError> {
    let manifest_path = dir.join("go.mod");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| InitError::ReadFile {
        path: manifest_path.clone(),
        source: e,
    })?;

    // Parse module name from go.mod
    let name = content
        .lines()
        .find(|line| line.starts_with("module "))
        .and_then(|line| line.strip_prefix("module "))
        .map(|s| {
            // Extract just the last path component as the name
            s.trim()
                .rsplit('/')
                .next()
                .map(sanitize_project_name)
                .unwrap_or_else(|| "unnamed".to_string())
        })
        .unwrap_or_else(|| {
            dir.file_name()
                .map(|s| sanitize_project_name(&s.to_string_lossy()))
                .unwrap_or_else(|| "unnamed".to_string())
        });

    let relative_path = dir
        .strip_prefix(root)
        .map_err(|_| InitError::InvalidPath {
            path: dir.to_path_buf(),
        })?
        .to_path_buf();

    // Skip root go.mod if it's at the workspace root
    if relative_path.as_os_str().is_empty() {
        return Ok(None);
    }

    // Standard Go targets
    let mut targets = BTreeMap::new();
    targets.insert(
        "build".to_string(),
        DetectedTarget {
            command: "go build ./...".to_string(),
            depends_on: vec!["^build".to_string()],
        },
    );
    targets.insert(
        "test".to_string(),
        DetectedTarget {
            command: "go test ./...".to_string(),
            depends_on: vec!["build".to_string()],
        },
    );
    targets.insert(
        "lint".to_string(),
        DetectedTarget {
            command: "golangci-lint run".to_string(),
            depends_on: vec![],
        },
    );

    let mut tags = vec![ProjectType::Go.as_str().to_string()];
    infer_tags_from_path(&relative_path, &mut tags);

    Ok(Some(DetectedProject {
        name,
        relative_path,
        absolute_path: dir.to_path_buf(),
        project_type: ProjectType::Go,
        targets,
        tags,
    }))
}

/// Detect a Python project from pyproject.toml.
fn detect_python_project(root: &Path, dir: &Path) -> Result<Option<DetectedProject>, InitError> {
    let manifest_path = dir.join("pyproject.toml");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| InitError::ReadFile {
        path: manifest_path.clone(),
        source: e,
    })?;

    #[derive(Deserialize)]
    struct PyProjectToml {
        project: Option<PyProject>,
        tool: Option<PyTool>,
    }

    #[derive(Deserialize)]
    struct PyProject {
        name: Option<String>,
    }

    #[derive(Deserialize)]
    struct PyTool {
        poetry: Option<PoetrySection>,
    }

    #[derive(Deserialize)]
    struct PoetrySection {
        name: Option<String>,
    }

    let pyproject: PyProjectToml = toml::from_str(&content).map_err(|e| InitError::ParseToml {
        path: manifest_path,
        source: e,
    })?;

    // Try to get name from [project] or [tool.poetry]
    let name = pyproject
        .project
        .and_then(|p| p.name)
        .or_else(|| pyproject.tool.and_then(|t| t.poetry.and_then(|p| p.name)))
        .map(|n| sanitize_project_name(&n))
        .unwrap_or_else(|| {
            dir.file_name()
                .map(|s| sanitize_project_name(&s.to_string_lossy()))
                .unwrap_or_else(|| "unnamed".to_string())
        });

    let relative_path = dir
        .strip_prefix(root)
        .map_err(|_| InitError::InvalidPath {
            path: dir.to_path_buf(),
        })?
        .to_path_buf();

    // Skip root pyproject.toml if it's at the workspace root
    if relative_path.as_os_str().is_empty() {
        return Ok(None);
    }

    // Standard Python targets
    let mut targets = BTreeMap::new();
    targets.insert(
        "test".to_string(),
        DetectedTarget {
            command: "pytest".to_string(),
            depends_on: vec![],
        },
    );
    targets.insert(
        "lint".to_string(),
        DetectedTarget {
            command: "ruff check .".to_string(),
            depends_on: vec![],
        },
    );

    let mut tags = vec![ProjectType::Python.as_str().to_string()];
    infer_tags_from_path(&relative_path, &mut tags);

    Ok(Some(DetectedProject {
        name,
        relative_path,
        absolute_path: dir.to_path_buf(),
        project_type: ProjectType::Python,
        targets,
        tags,
    }))
}

/// Sanitize a name to be a valid project name (lowercase, no special chars).
fn sanitize_project_name(name: &str) -> String {
    name.chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c == '-' || c == '_' {
                Some(c)
            } else if c == ' ' || c == '/' || c == '@' {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Infer tags from a project's relative path.
fn infer_tags_from_path(path: &Path, tags: &mut Vec<String>) {
    let path_str = path.to_string_lossy().to_lowercase();
    if path_str.contains("app") {
        tags.push("app".to_string());
    } else if path_str.contains("lib") || path_str.contains("package") {
        tags.push("lib".to_string());
    }
}

/// Generate workspace glob patterns from detected projects.
pub fn generate_workspace_patterns(projects: &[DetectedProject]) -> Vec<String> {
    let mut patterns = HashSet::new();

    for project in projects {
        if let Some(first_component) = project.relative_path.components().next() {
            let dir = first_component.as_os_str().to_string_lossy();
            patterns.insert(format!("{dir}/*"));
        }
    }

    let mut sorted: Vec<_> = patterns.into_iter().collect();
    sorted.sort();
    sorted
}

/// Generate the root guild.toml content.
pub fn generate_workspace_toml(name: &str, patterns: &[String]) -> String {
    let mut toml = String::new();
    toml.push_str("[workspace]\n");
    toml.push_str(&format!("name = \"{name}\"\n"));
    toml.push_str("projects = [");
    for (i, pattern) in patterns.iter().enumerate() {
        if i > 0 {
            toml.push_str(", ");
        }
        toml.push_str(&format!("\"{pattern}\""));
    }
    toml.push_str("]\n");
    toml
}

/// Generate a project guild.toml content.
pub fn generate_project_toml(project: &DetectedProject) -> String {
    let mut toml = String::new();

    // [project] section
    toml.push_str("[project]\n");
    toml.push_str(&format!("name = \"{}\"\n", project.name));
    if !project.tags.is_empty() {
        toml.push_str("tags = [");
        for (i, tag) in project.tags.iter().enumerate() {
            if i > 0 {
                toml.push_str(", ");
            }
            toml.push_str(&format!("\"{tag}\""));
        }
        toml.push_str("]\n");
    }

    // [targets.*] sections
    for (name, target) in &project.targets {
        toml.push_str(&format!("\n[targets.{name}]\n"));
        toml.push_str(&format!("command = \"{}\"\n", target.command));
        if !target.depends_on.is_empty() {
            toml.push_str("depends_on = [");
            for (i, dep) in target.depends_on.iter().enumerate() {
                if i > 0 {
                    toml.push_str(", ");
                }
                toml.push_str(&format!("\"{dep}\""));
            }
            toml.push_str("]\n");
        }
    }

    toml
}

/// Initialize a workspace with guild.toml files.
pub fn init_workspace(
    root: &Path,
    workspace_name: &str,
    yes: bool,
    reader: &mut dyn BufRead,
    writer: &mut dyn IoWrite,
) -> Result<InitResult, InitError> {
    let projects = detect_projects(root)?;
    let patterns = generate_workspace_patterns(&projects);

    let mut result = InitResult {
        written: Vec::new(),
        skipped: Vec::new(),
    };

    // Generate and write root guild.toml
    let root_toml_path = root.join("guild.toml");
    let workspace_toml = generate_workspace_toml(workspace_name, &patterns);

    if root_toml_path.exists() {
        print_warning(&format!(
            "Skipping {} (already exists)",
            root_toml_path.display()
        ));
        result.skipped.push(root_toml_path);
    } else {
        let should_write = if yes {
            true
        } else {
            prompt_confirm(
                &format!("Create {}?", root_toml_path.display()),
                &workspace_toml,
                reader,
                writer,
            )?
        };

        if should_write {
            std::fs::write(&root_toml_path, &workspace_toml).map_err(|e| InitError::WriteFile {
                path: root_toml_path.clone(),
                source: e,
            })?;
            print_success(&format!("Created {}", root_toml_path.display()));
            result.written.push(root_toml_path);
        } else {
            result.skipped.push(root_toml_path);
        }
    }

    // Generate and write per-project guild.toml files
    for project in &projects {
        let project_toml_path = project.absolute_path.join("guild.toml");
        let project_toml = generate_project_toml(project);

        if project_toml_path.exists() {
            print_warning(&format!(
                "Skipping {} (already exists)",
                project_toml_path.display()
            ));
            result.skipped.push(project_toml_path);
        } else {
            let should_write = if yes {
                true
            } else {
                prompt_confirm(
                    &format!("Create {}?", project_toml_path.display()),
                    &project_toml,
                    reader,
                    writer,
                )?
            };

            if should_write {
                std::fs::write(&project_toml_path, &project_toml).map_err(|e| {
                    InitError::WriteFile {
                        path: project_toml_path.clone(),
                        source: e,
                    }
                })?;
                print_success(&format!("Created {}", project_toml_path.display()));
                result.written.push(project_toml_path);
            } else {
                result.skipped.push(project_toml_path);
            }
        }
    }

    Ok(result)
}

/// Prompt the user to confirm writing a file.
fn prompt_confirm(
    prompt: &str,
    content: &str,
    reader: &mut dyn BufRead,
    writer: &mut dyn IoWrite,
) -> Result<bool, InitError> {
    writeln!(writer, "\n{prompt}").map_err(|e| InitError::Io { source: e })?;
    writeln!(writer, "---").map_err(|e| InitError::Io { source: e })?;
    for line in content.lines() {
        writeln!(writer, "  {line}").map_err(|e| InitError::Io { source: e })?;
    }
    writeln!(writer, "---").map_err(|e| InitError::Io { source: e })?;
    write!(writer, "[y/n] ").map_err(|e| InitError::Io { source: e })?;
    writer.flush().map_err(|e| InitError::Io { source: e })?;

    let mut response = String::new();
    reader
        .read_line(&mut response)
        .map_err(|e| InitError::Io { source: e })?;

    Ok(response.trim().eq_ignore_ascii_case("y") || response.trim().eq_ignore_ascii_case("yes"))
}

/// Run init with stdin/stdout for interactive mode.
pub fn run_init(root: &Path, workspace_name: &str, yes: bool) -> Result<InitResult, InitError> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    init_workspace(root, workspace_name, yes, &mut reader, &mut writer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();

        // Create apps/web with package.json
        let web_dir = dir.path().join("apps/web");
        std::fs::create_dir_all(&web_dir).unwrap();
        std::fs::write(
            web_dir.join("package.json"),
            r#"{"name": "web-app", "scripts": {"build": "vite build", "test": "vitest", "lint": "eslint ."}}"#,
        )
        .unwrap();

        // Create libs/core with Cargo.toml
        let core_dir = dir.path().join("libs/core");
        std::fs::create_dir_all(&core_dir).unwrap();
        std::fs::write(
            core_dir.join("Cargo.toml"),
            r#"[package]
name = "core-lib"
version = "0.1.0"
"#,
        )
        .unwrap();

        dir
    }

    #[test]
    fn test_detect_node_project() {
        let dir = TempDir::new().unwrap();
        let project_dir = dir.path().join("apps/my-app");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("package.json"),
            r#"{"name": "@scope/my-app", "scripts": {"build": "tsc", "test": "jest"}}"#,
        )
        .unwrap();

        let projects = detect_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "scope-my-app");
        assert!(projects[0].targets.contains_key("build"));
        assert!(projects[0].targets.contains_key("test"));
    }

    #[test]
    fn test_detect_rust_project() {
        let dir = TempDir::new().unwrap();
        let project_dir = dir.path().join("libs/my-lib");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("Cargo.toml"),
            r#"[package]
name = "my-lib"
version = "0.1.0"
"#,
        )
        .unwrap();

        let projects = detect_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my-lib");
        assert!(projects[0].targets.contains_key("build"));
        assert!(projects[0].targets.contains_key("test"));
        assert!(projects[0].targets.contains_key("lint"));
    }

    #[test]
    fn test_skip_workspace_only_cargo_toml() {
        let dir = TempDir::new().unwrap();
        let project_dir = dir.path().join("libs/my-lib");
        std::fs::create_dir_all(&project_dir).unwrap();

        // Workspace-only Cargo.toml at root
        std::fs::write(
            project_dir.join("Cargo.toml"),
            r#"[workspace]
members = ["crates/*"]
"#,
        )
        .unwrap();

        let projects = detect_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 0);
    }

    #[test]
    fn test_detect_go_project() {
        let dir = TempDir::new().unwrap();
        let project_dir = dir.path().join("services/api");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("go.mod"),
            "module github.com/example/api\n\ngo 1.21\n",
        )
        .unwrap();

        let projects = detect_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "api");
        assert!(projects[0].targets.contains_key("build"));
        assert!(projects[0].targets.contains_key("test"));
    }

    #[test]
    fn test_detect_python_project() {
        let dir = TempDir::new().unwrap();
        let project_dir = dir.path().join("packages/my-pkg");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("pyproject.toml"),
            r#"[project]
name = "my-pkg"
version = "0.1.0"
"#,
        )
        .unwrap();

        let projects = detect_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my-pkg");
        assert!(projects[0].targets.contains_key("test"));
        assert!(projects[0].targets.contains_key("lint"));
    }

    #[test]
    fn test_generate_workspace_patterns() {
        let projects = vec![
            DetectedProject {
                name: "web".to_string(),
                relative_path: PathBuf::from("apps/web"),
                absolute_path: PathBuf::from("/tmp/apps/web"),
                project_type: ProjectType::Node,
                targets: BTreeMap::new(),
                tags: vec![],
            },
            DetectedProject {
                name: "core".to_string(),
                relative_path: PathBuf::from("libs/core"),
                absolute_path: PathBuf::from("/tmp/libs/core"),
                project_type: ProjectType::Rust,
                targets: BTreeMap::new(),
                tags: vec![],
            },
        ];

        let patterns = generate_workspace_patterns(&projects);
        assert_eq!(patterns, vec!["apps/*", "libs/*"]);
    }

    #[test]
    fn test_generate_workspace_toml() {
        let toml =
            generate_workspace_toml("my-monorepo", &["apps/*".to_string(), "libs/*".to_string()]);
        assert!(toml.contains("[workspace]"));
        assert!(toml.contains("name = \"my-monorepo\""));
        assert!(toml.contains("projects = [\"apps/*\", \"libs/*\"]"));
    }

    #[test]
    fn test_generate_project_toml() {
        let mut targets = BTreeMap::new();
        targets.insert(
            "build".to_string(),
            DetectedTarget {
                command: "npm run build".to_string(),
                depends_on: vec!["^build".to_string()],
            },
        );

        let project = DetectedProject {
            name: "my-app".to_string(),
            relative_path: PathBuf::from("apps/my-app"),
            absolute_path: PathBuf::from("/tmp/apps/my-app"),
            project_type: ProjectType::Node,
            targets,
            tags: vec!["node".to_string(), "app".to_string()],
        };

        let toml = generate_project_toml(&project);
        assert!(toml.contains("[project]"));
        assert!(toml.contains("name = \"my-app\""));
        assert!(toml.contains("tags = [\"node\", \"app\"]"));
        assert!(toml.contains("[targets.build]"));
        assert!(toml.contains("command = \"npm run build\""));
        assert!(toml.contains("depends_on = [\"^build\"]"));
    }

    #[test]
    fn test_init_workspace_yes_mode() {
        let dir = create_test_workspace();
        let mut reader = Cursor::new(Vec::new());
        let mut writer = Vec::new();

        let result =
            init_workspace(dir.path(), "test-workspace", true, &mut reader, &mut writer).unwrap();

        assert_eq!(result.written.len(), 3); // root + 2 projects
        assert!(dir.path().join("guild.toml").exists());
        assert!(dir.path().join("apps/web/guild.toml").exists());
        assert!(dir.path().join("libs/core/guild.toml").exists());
    }

    #[test]
    fn test_init_workspace_skips_existing() {
        let dir = create_test_workspace();

        // Pre-create guild.toml
        std::fs::write(
            dir.path().join("guild.toml"),
            "[workspace]\nname = \"existing\"\nprojects = []\n",
        )
        .unwrap();

        let mut reader = Cursor::new(Vec::new());
        let mut writer = Vec::new();

        let result =
            init_workspace(dir.path(), "test-workspace", true, &mut reader, &mut writer).unwrap();

        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].ends_with("guild.toml"));
        // Should still create project files
        assert_eq!(result.written.len(), 2);
    }

    #[test]
    fn test_sanitize_project_name() {
        assert_eq!(sanitize_project_name("My App"), "my-app");
        assert_eq!(sanitize_project_name("@scope/pkg"), "scope-pkg");
        assert_eq!(sanitize_project_name("my_lib-v2"), "my_lib-v2");
        assert_eq!(sanitize_project_name("UPPERCASE"), "uppercase");
    }
}
