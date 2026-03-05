use std::path::Path;

use colored::Colorize;

use crate::affected::{compute_affected, get_changed_files};
use crate::config::{TargetName, WorkspaceConfig};
use crate::discovery::{discover_projects, find_workspace_root};
use crate::error::AffectedError;
use crate::graph::{ProjectGraph, TaskGraph};
use crate::runner::{RunResult, TaskRunner};

/// Run a target only on affected projects (changed + their dependents).
///
/// Detects which projects have changed since the base branch and runs the target
/// on those projects plus any projects that transitively depend on them.
pub async fn run_affected(
    cwd: &Path,
    target: &str,
    base_branch: &str,
) -> Result<RunResult, AffectedError> {
    // Parse target name
    let target_name: TargetName = target.parse().map_err(|e| AffectedError::InvalidTarget {
        target: target.to_string(),
        reason: format!("{e}"),
    })?;

    // Discover workspace
    let root = find_workspace_root(cwd).map_err(|e| AffectedError::WorkspaceNotFound {
        path: cwd.to_path_buf(),
        reason: format!("{e}"),
    })?;

    let workspace = WorkspaceConfig::from_file(&root.join("guild.toml")).map_err(|e| {
        AffectedError::ConfigError {
            path: root.join("guild.toml"),
            reason: format!("{e}"),
        }
    })?;

    // Discover all projects
    let all_projects = discover_projects(&workspace).map_err(|e| AffectedError::ConfigError {
        path: root.clone(),
        reason: format!("{e}"),
    })?;

    if all_projects.is_empty() {
        return Ok(RunResult {
            success_count: 0,
            failure_count: 0,
            skipped_count: 0,
            cached_count: 0,
            task_results: vec![],
            total_duration: std::time::Duration::ZERO,
        });
    }

    // Build full project graph
    let full_project_graph =
        ProjectGraph::build(all_projects.clone()).map_err(|e| AffectedError::GraphError {
            reason: format!("{e}"),
        })?;

    // Get changed files from git
    let changed_files = get_changed_files(&root, base_branch)?;

    // Compute affected projects
    let affected = compute_affected(&changed_files, &all_projects, &full_project_graph);

    if affected.all.is_empty() {
        println!(
            "\n{} No projects affected since '{}'\n",
            "guild".cyan().bold(),
            base_branch
        );
        return Ok(RunResult {
            success_count: 0,
            failure_count: 0,
            skipped_count: 0,
            cached_count: 0,
            task_results: vec![],
            total_duration: std::time::Duration::ZERO,
        });
    }

    // Filter projects to only affected ones
    let affected_projects: Vec<_> = all_projects
        .into_iter()
        .filter(|p| affected.all.contains(p.name()))
        .collect();

    // Build project graph from affected projects only
    let affected_graph =
        ProjectGraph::build(affected_projects).map_err(|e| AffectedError::GraphError {
            reason: format!("{e}"),
        })?;

    // Build task graph
    let task_graph =
        TaskGraph::build(&affected_graph, &target_name).map_err(|e| AffectedError::GraphError {
            reason: format!("{e}"),
        })?;

    let task_count = task_graph.len();

    if task_count == 0 {
        println!(
            "{} No affected projects have target '{target}'",
            "warning:".yellow().bold()
        );
        return Ok(RunResult {
            success_count: 0,
            failure_count: 0,
            skipped_count: 0,
            cached_count: 0,
            task_results: vec![],
            total_duration: std::time::Duration::ZERO,
        });
    }

    // Print header with affected info
    println!(
        "\n{} Running {} task{} for target '{}' on {} affected project{}\n",
        "guild".cyan().bold(),
        task_count,
        if task_count == 1 { "" } else { "s" },
        target,
        affected.all.len(),
        if affected.all.len() == 1 { "" } else { "s" }
    );

    // Show breakdown of changed vs dependents
    if !affected.changed.is_empty() {
        let changed_names: Vec<String> = affected.changed.iter().map(|n| n.to_string()).collect();
        println!("  {} Changed: {}", "~".yellow(), changed_names.join(", "));
    }
    if !affected.dependents.is_empty() {
        let dep_names: Vec<String> = affected.dependents.iter().map(|n| n.to_string()).collect();
        println!("  {} Dependents: {}", "~".cyan(), dep_names.join(", "));
    }
    println!();

    // Create runner with reasonable defaults
    let concurrency = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let runner = TaskRunner::new(concurrency, root);

    // Execute
    let result =
        runner
            .run(task_graph, &affected_graph)
            .await
            .map_err(|e| AffectedError::GraphError {
                reason: format!("{e}"),
            })?;

    // Print summary
    print_summary(&result, target);

    Ok(result)
}

/// Print a summary of the run result.
fn print_summary(result: &RunResult, target: &str) {
    println!();

    let duration_str = format!("{:.2}s", result.total_duration.as_secs_f64());

    if result.is_success() {
        if result.success_count == 0 {
            println!(
                "{} No tasks executed for target '{target}'",
                "warning:".yellow().bold()
            );
        } else {
            println!(
                "{} {} {} completed successfully in {}",
                "Success".green().bold(),
                result.success_count,
                if result.success_count == 1 {
                    "task"
                } else {
                    "tasks"
                },
                duration_str
            );
        }
    } else {
        let mut parts = Vec::new();

        if result.success_count > 0 {
            parts.push(format!("{} {}", result.success_count, "succeeded".green()));
        }

        parts.push(format!("{} {}", result.failure_count, "failed".red()));

        if result.skipped_count > 0 {
            parts.push(format!("{} {}", result.skipped_count, "skipped".yellow()));
        }

        println!(
            "{} {} in {}",
            "Failed".red().bold(),
            parts.join(", "),
            duration_str
        );
    }
}
