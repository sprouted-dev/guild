use std::collections::HashSet;
use std::path::Path;

use colored::Colorize;

use crate::config::{ProjectName, TargetName, WorkspaceConfig};
use crate::discovery::{discover_projects, find_workspace_root};
use crate::error::RunnerError;
use crate::graph::{ProjectGraph, TaskGraph};
use crate::runner::{RunResult, TaskRunner};

/// Run a target across the workspace or for a specific project.
///
/// If `project` is `Some`, scopes execution to that project and its upstream dependencies.
/// Returns the run result with success/failure counts.
pub async fn run_target(
    cwd: &Path,
    target: &str,
    project: Option<&str>,
) -> Result<RunResult, RunnerError> {
    // Parse target name
    let target_name: TargetName = target.parse().map_err(|e| RunnerError::InvalidTarget {
        target: target.to_string(),
        reason: format!("{e}"),
    })?;

    // Discover workspace
    let root = find_workspace_root(cwd).map_err(|e| RunnerError::WorkspaceNotFound {
        path: cwd.to_path_buf(),
        reason: format!("{e}"),
    })?;

    let workspace = WorkspaceConfig::from_file(&root.join("guild.toml")).map_err(|e| {
        RunnerError::ConfigError {
            path: root.join("guild.toml"),
            reason: format!("{e}"),
        }
    })?;

    // Discover projects
    let all_projects = discover_projects(&workspace).map_err(|e| RunnerError::ConfigError {
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

    // Build project graph from all projects
    let full_project_graph =
        ProjectGraph::build(all_projects.clone()).map_err(|e| RunnerError::GraphError {
            reason: format!("{e}"),
        })?;

    // Optionally filter to specific project + its dependencies
    let (project_graph, scoped_project) = if let Some(project_name) = project {
        let project_name: ProjectName =
            project_name
                .parse()
                .map_err(|e| RunnerError::InvalidTarget {
                    target: project_name.to_string(),
                    reason: format!("{e}"),
                })?;

        // Check that the project exists
        if full_project_graph.get(&project_name).is_none() {
            return Err(RunnerError::ProjectNotFound {
                name: project_name.to_string(),
            });
        }

        // Collect the project and all its transitive upstream dependencies
        let required_projects = collect_upstream_projects(&full_project_graph, &project_name);

        // Filter the projects to only those required
        let filtered_projects: Vec<_> = all_projects
            .into_iter()
            .filter(|p| required_projects.contains(p.name()))
            .collect();

        let filtered_graph =
            ProjectGraph::build(filtered_projects).map_err(|e| RunnerError::GraphError {
                reason: format!("{e}"),
            })?;

        (filtered_graph, Some(project_name))
    } else {
        (full_project_graph, None)
    };

    // Build task graph
    let task_graph =
        TaskGraph::build(&project_graph, &target_name).map_err(|e| RunnerError::GraphError {
            reason: format!("{e}"),
        })?;

    let task_count = task_graph.len();

    if task_count == 0 {
        let scope_msg = scoped_project
            .map(|p| format!(" for project '{p}'"))
            .unwrap_or_default();
        println!(
            "{} No projects have target '{target}'{scope_msg}",
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

    // Print header
    let scope_msg = scoped_project
        .as_ref()
        .map(|p| format!(" (scoped to {p})"))
        .unwrap_or_default();
    println!(
        "\n{} Running {} task{} for target '{}'{}\n",
        "guild".cyan().bold(),
        task_count,
        if task_count == 1 { "" } else { "s" },
        target,
        scope_msg
    );

    // Create runner with reasonable defaults
    let concurrency = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let runner = TaskRunner::new(concurrency, root);

    // Execute
    let result = runner.run(task_graph, &project_graph).await?;

    // Print summary
    print_summary(&result, target);

    Ok(result)
}

/// Collect a project and all its transitive upstream dependencies.
fn collect_upstream_projects(graph: &ProjectGraph, start: &ProjectName) -> HashSet<ProjectName> {
    let mut result = HashSet::new();
    let mut to_visit = vec![start.clone()];

    while let Some(name) = to_visit.pop() {
        if result.contains(&name) {
            continue;
        }
        result.insert(name.clone());

        if let Some(deps) = graph.dependencies(&name) {
            for dep in deps {
                if !result.contains(dep) {
                    to_visit.push(dep.clone());
                }
            }
        }
    }

    result
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
