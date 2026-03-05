use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{DiffOptions, Repository};

use crate::config::{ProjectConfig, ProjectName};
use crate::error::AffectedError;
use crate::graph::ProjectGraph;

/// Result of affected project detection.
#[derive(Debug)]
pub struct AffectedResult {
    /// Projects that have changed files.
    pub changed: HashSet<ProjectName>,
    /// Projects that transitively depend on changed projects.
    pub dependents: HashSet<ProjectName>,
    /// All affected projects (changed + dependents).
    pub all: HashSet<ProjectName>,
}

/// Get the list of files changed since the merge-base with the given base branch.
///
/// This includes:
/// - Files changed in commits since diverging from base
/// - Staged changes (index)
/// - Unstaged working directory changes
pub fn get_changed_files(
    repo_root: &Path,
    base_branch: &str,
) -> Result<Vec<PathBuf>, AffectedError> {
    let repo = Repository::open(repo_root).map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            AffectedError::NotAGitRepo {
                path: repo_root.to_path_buf(),
            }
        } else {
            AffectedError::Git {
                message: e.message().to_string(),
            }
        }
    })?;

    let mut changed_files = HashSet::new();

    // Find the merge-base with the base branch
    let base_commit = find_merge_base(&repo, base_branch)?;

    // Get changes from merge-base to HEAD
    if let Some(ref base) = base_commit {
        let base_tree: git2::Tree<'_> = base.tree().map_err(|e| AffectedError::Git {
            message: format!("failed to get base tree: {e}"),
        })?;

        let head = repo.head().map_err(|e| AffectedError::Git {
            message: format!("failed to get HEAD: {e}"),
        })?;

        let head_commit = head.peel_to_commit().map_err(|e| AffectedError::Git {
            message: format!("failed to get HEAD commit: {e}"),
        })?;

        let head_tree = head_commit.tree().map_err(|e| AffectedError::Git {
            message: format!("failed to get HEAD tree: {e}"),
        })?;

        let mut opts = DiffOptions::new();
        let diff = repo
            .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut opts))
            .map_err(|e| AffectedError::Git {
                message: format!("failed to diff trees: {e}"),
            })?;

        for delta in diff.deltas() {
            if let Some(path) = delta.new_file().path() {
                changed_files.insert(repo_root.join(path));
            }
            if let Some(path) = delta.old_file().path() {
                changed_files.insert(repo_root.join(path));
            }
        }
    }

    // Get staged changes (index vs HEAD)
    let head = repo.head().ok();
    let head_tree = head.as_ref().and_then(|h| h.peel_to_tree().ok());

    let mut opts = DiffOptions::new();
    let diff = repo
        .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
        .map_err(|e| AffectedError::Git {
            message: format!("failed to diff index: {e}"),
        })?;

    for delta in diff.deltas() {
        if let Some(path) = delta.new_file().path() {
            changed_files.insert(repo_root.join(path));
        }
        if let Some(path) = delta.old_file().path() {
            changed_files.insert(repo_root.join(path));
        }
    }

    // Get unstaged working directory changes (workdir vs index)
    let mut opts = DiffOptions::new();
    let diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .map_err(|e| AffectedError::Git {
            message: format!("failed to diff workdir: {e}"),
        })?;

    for delta in diff.deltas() {
        if let Some(path) = delta.new_file().path() {
            changed_files.insert(repo_root.join(path));
        }
        if let Some(path) = delta.old_file().path() {
            changed_files.insert(repo_root.join(path));
        }
    }

    Ok(changed_files.into_iter().collect())
}

/// Find the merge-base commit between HEAD and the given base branch.
fn find_merge_base<'a>(
    repo: &'a Repository,
    base_branch: &str,
) -> Result<Option<git2::Commit<'a>>, AffectedError> {
    // Try to find the base branch as a local branch first
    let base_ref = repo
        .find_branch(base_branch, git2::BranchType::Local)
        .or_else(|_| {
            // Try as a remote branch (origin/main)
            let remote_name = format!("origin/{base_branch}");
            repo.find_branch(&remote_name, git2::BranchType::Remote)
        })
        .map_err(|_| AffectedError::BaseBranchNotFound {
            branch: base_branch.to_string(),
        })?;

    let base_oid = base_ref
        .get()
        .target()
        .ok_or_else(|| AffectedError::BaseBranchNotFound {
            branch: base_branch.to_string(),
        })?;

    let head = repo.head().map_err(|e| AffectedError::Git {
        message: format!("failed to get HEAD: {e}"),
    })?;

    let head_oid = head.target().ok_or_else(|| AffectedError::Git {
        message: "HEAD has no target".to_string(),
    })?;

    // Find merge-base
    let merge_base = repo
        .merge_base(head_oid, base_oid)
        .map_err(|e| AffectedError::Git {
            message: format!("failed to find merge-base: {e}"),
        })?;

    let commit = repo
        .find_commit(merge_base)
        .map_err(|e| AffectedError::Git {
            message: format!("failed to find merge-base commit: {e}"),
        })?;

    Ok(Some(commit))
}

/// Map changed files to the projects that contain them.
pub fn map_files_to_projects(
    changed_files: &[PathBuf],
    projects: &[ProjectConfig],
) -> HashSet<ProjectName> {
    let mut affected = HashSet::new();

    for file in changed_files {
        for project in projects {
            if file.starts_with(project.root()) {
                affected.insert(project.name().clone());
                break;
            }
        }
    }

    affected
}

/// Expand the set of changed projects to include all downstream dependents.
///
/// A project is "downstream" if it depends (directly or transitively) on a changed project.
pub fn expand_to_dependents(
    changed: &HashSet<ProjectName>,
    graph: &ProjectGraph,
) -> HashSet<ProjectName> {
    // Build a reverse dependency map: project -> projects that depend on it
    let mut reverse_deps: std::collections::HashMap<ProjectName, HashSet<ProjectName>> =
        std::collections::HashMap::new();

    for name in graph.project_names() {
        if let Some(deps) = graph.dependencies(name) {
            for dep in deps {
                reverse_deps
                    .entry(dep.clone())
                    .or_default()
                    .insert(name.clone());
            }
        }
    }

    // BFS to find all transitive dependents
    let mut dependents = HashSet::new();
    let mut to_visit: Vec<ProjectName> = changed.iter().cloned().collect();

    while let Some(name) = to_visit.pop() {
        if let Some(deps) = reverse_deps.get(&name) {
            for dep in deps {
                if !dependents.contains(dep) && !changed.contains(dep) {
                    dependents.insert(dep.clone());
                    to_visit.push(dep.clone());
                }
            }
        }
    }

    dependents
}

/// Compute the full set of affected projects.
pub fn compute_affected(
    changed_files: &[PathBuf],
    projects: &[ProjectConfig],
    graph: &ProjectGraph,
) -> AffectedResult {
    let changed = map_files_to_projects(changed_files, projects);
    let dependents = expand_to_dependents(&changed, graph);

    let mut all = changed.clone();
    all.extend(dependents.iter().cloned());

    AffectedResult {
        changed,
        dependents,
        all,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_project(name: &str, root: &str, deps: &[&str]) -> ProjectConfig {
        let deps_str = if deps.is_empty() {
            String::new()
        } else {
            let dep_list: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
            format!("depends_on = [{}]", dep_list.join(", "))
        };
        let toml = format!(
            "[project]\nname = \"{name}\"\n{deps_str}\n\n[targets.build]\ncommand = \"echo build\"\n"
        );
        ProjectConfig::from_str(&toml, PathBuf::from(root)).unwrap()
    }

    #[test]
    fn test_map_files_to_projects() {
        let projects = vec![
            make_project("app", "/workspace/apps/app", &[]),
            make_project("lib", "/workspace/libs/lib", &[]),
        ];

        let changed_files = vec![
            PathBuf::from("/workspace/apps/app/src/main.rs"),
            PathBuf::from("/workspace/libs/lib/src/lib.rs"),
            PathBuf::from("/workspace/README.md"), // not in any project
        ];

        let affected = map_files_to_projects(&changed_files, &projects);

        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&"app".parse().unwrap()));
        assert!(affected.contains(&"lib".parse().unwrap()));
    }

    #[test]
    fn test_map_files_single_project() {
        let projects = vec![
            make_project("app", "/workspace/apps/app", &[]),
            make_project("lib", "/workspace/libs/lib", &[]),
        ];

        let changed_files = vec![PathBuf::from("/workspace/apps/app/src/main.rs")];

        let affected = map_files_to_projects(&changed_files, &projects);

        assert_eq!(affected.len(), 1);
        assert!(affected.contains(&"app".parse().unwrap()));
    }

    #[test]
    fn test_map_files_no_projects() {
        let projects = vec![
            make_project("app", "/workspace/apps/app", &[]),
            make_project("lib", "/workspace/libs/lib", &[]),
        ];

        let changed_files = vec![PathBuf::from("/workspace/README.md")];

        let affected = map_files_to_projects(&changed_files, &projects);

        assert!(affected.is_empty());
    }

    #[test]
    fn test_expand_to_dependents() {
        // app depends on lib
        let projects = vec![
            make_project("app", "/workspace/apps/app", &["lib"]),
            make_project("lib", "/workspace/libs/lib", &[]),
        ];
        let graph = ProjectGraph::build(projects).unwrap();

        // lib changed -> app should be affected as dependent
        let changed: HashSet<_> = vec!["lib".parse().unwrap()].into_iter().collect();
        let dependents = expand_to_dependents(&changed, &graph);

        assert_eq!(dependents.len(), 1);
        assert!(dependents.contains(&"app".parse().unwrap()));
    }

    #[test]
    fn test_expand_to_dependents_transitive() {
        // app depends on lib-a, lib-a depends on core
        let projects = vec![
            make_project("app", "/workspace/apps/app", &["lib-a"]),
            make_project("lib-a", "/workspace/libs/lib-a", &["core"]),
            make_project("core", "/workspace/libs/core", &[]),
        ];
        let graph = ProjectGraph::build(projects).unwrap();

        // core changed -> lib-a and app should be affected
        let changed: HashSet<_> = vec!["core".parse().unwrap()].into_iter().collect();
        let dependents = expand_to_dependents(&changed, &graph);

        assert_eq!(dependents.len(), 2);
        assert!(dependents.contains(&"lib-a".parse().unwrap()));
        assert!(dependents.contains(&"app".parse().unwrap()));
    }

    #[test]
    fn test_expand_to_dependents_diamond() {
        // app depends on lib-a and lib-b, both depend on core
        let projects = vec![
            make_project("app", "/workspace/apps/app", &["lib-a", "lib-b"]),
            make_project("lib-a", "/workspace/libs/lib-a", &["core"]),
            make_project("lib-b", "/workspace/libs/lib-b", &["core"]),
            make_project("core", "/workspace/libs/core", &[]),
        ];
        let graph = ProjectGraph::build(projects).unwrap();

        // core changed -> lib-a, lib-b, and app should be affected
        let changed: HashSet<_> = vec!["core".parse().unwrap()].into_iter().collect();
        let dependents = expand_to_dependents(&changed, &graph);

        assert_eq!(dependents.len(), 3);
        assert!(dependents.contains(&"lib-a".parse().unwrap()));
        assert!(dependents.contains(&"lib-b".parse().unwrap()));
        assert!(dependents.contains(&"app".parse().unwrap()));
    }

    #[test]
    fn test_expand_no_dependents() {
        let projects = vec![
            make_project("app", "/workspace/apps/app", &["lib"]),
            make_project("lib", "/workspace/libs/lib", &[]),
        ];
        let graph = ProjectGraph::build(projects).unwrap();

        // app changed -> no dependents (nothing depends on app)
        let changed: HashSet<_> = vec!["app".parse().unwrap()].into_iter().collect();
        let dependents = expand_to_dependents(&changed, &graph);

        assert!(dependents.is_empty());
    }

    #[test]
    fn test_compute_affected() {
        // app depends on lib
        let projects = vec![
            make_project("app", "/workspace/apps/app", &["lib"]),
            make_project("lib", "/workspace/libs/lib", &[]),
        ];
        let graph = ProjectGraph::build(projects.clone()).unwrap();

        let changed_files = vec![PathBuf::from("/workspace/libs/lib/src/lib.rs")];

        let result = compute_affected(&changed_files, &projects, &graph);

        assert_eq!(result.changed.len(), 1);
        assert!(result.changed.contains(&"lib".parse().unwrap()));

        assert_eq!(result.dependents.len(), 1);
        assert!(result.dependents.contains(&"app".parse().unwrap()));

        assert_eq!(result.all.len(), 2);
        assert!(result.all.contains(&"lib".parse().unwrap()));
        assert!(result.all.contains(&"app".parse().unwrap()));
    }
}
