use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use colored::{Color, Colorize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::error::RunnerError;
use crate::graph::{ProjectGraph, TaskGraph, TaskId};

/// Controls behavior when a task fails.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunMode {
    /// Stop scheduling new tasks after first failure, but wait for running tasks.
    #[default]
    FailFast,
    /// Continue executing tasks even after failures.
    Continue,
}

/// Result of a single task execution.
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// The task that was executed.
    pub task_id: TaskId,
    /// Whether the task succeeded.
    pub success: bool,
    /// Exit code, if the process exited normally.
    pub exit_code: Option<i32>,
    /// Duration of the task execution.
    pub duration: Duration,
}

/// Aggregate result of running all tasks.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Number of tasks that succeeded.
    pub success_count: usize,
    /// Number of tasks that failed.
    pub failure_count: usize,
    /// Number of tasks that were skipped (due to fail-fast).
    pub skipped_count: usize,
    /// Individual task results in completion order.
    pub task_results: Vec<TaskResult>,
    /// Total duration of the run.
    pub total_duration: Duration,
}

impl RunResult {
    /// Returns true if all executed tasks succeeded.
    pub fn is_success(&self) -> bool {
        self.failure_count == 0
    }
}

/// Event sent from task workers to the orchestrator.
#[derive(Debug)]
enum TaskEvent {
    /// A task has completed.
    Completed { task_id: TaskId, result: TaskResult },
}

/// Colors for project prefixes (cycled through).
const PROJECT_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Red,
];

/// Parallel task execution engine.
#[derive(Debug)]
pub struct TaskRunner {
    /// Maximum number of concurrent tasks.
    concurrency: usize,
    /// Behavior when a task fails.
    run_mode: RunMode,
    /// Working directory for tasks (workspace root).
    working_dir: PathBuf,
}

impl TaskRunner {
    /// Create a new task runner with the given concurrency limit.
    pub fn new(concurrency: usize, working_dir: PathBuf) -> Self {
        Self {
            concurrency: concurrency.max(1),
            run_mode: RunMode::default(),
            working_dir,
        }
    }

    /// Set the run mode (fail-fast or continue).
    pub fn with_run_mode(mut self, mode: RunMode) -> Self {
        self.run_mode = mode;
        self
    }

    /// Run all tasks in the task graph.
    pub async fn run(
        &self,
        mut task_graph: TaskGraph,
        project_graph: &ProjectGraph,
    ) -> Result<RunResult, RunnerError> {
        let start_time = Instant::now();
        let total_tasks = task_graph.len();

        if total_tasks == 0 {
            return Ok(RunResult {
                success_count: 0,
                failure_count: 0,
                skipped_count: 0,
                task_results: vec![],
                total_duration: start_time.elapsed(),
            });
        }

        // Build project name -> color mapping
        let mut color_map: HashMap<String, Color> = HashMap::new();
        for (idx, name) in project_graph.project_names().enumerate() {
            color_map.insert(name.to_string(), PROJECT_COLORS[idx % PROJECT_COLORS.len()]);
        }

        // Build project name -> root path mapping
        let mut project_roots: HashMap<String, PathBuf> = HashMap::new();
        for name in project_graph.project_names() {
            if let Some(project) = project_graph.get(name) {
                project_roots.insert(name.to_string(), project.root().to_path_buf());
            }
        }

        // Build task -> command mapping
        let mut task_commands: HashMap<TaskId, (String, PathBuf)> = HashMap::new();
        for task_id in task_graph.tasks() {
            let project_name = task_id.project().to_string();
            let target_name = task_id.target();
            if let Some(project) = project_graph.get(task_id.project())
                && let Some(target) = project.targets().get(target_name)
            {
                let root = project_roots
                    .get(&project_name)
                    .cloned()
                    .unwrap_or_else(|| self.working_dir.clone());
                task_commands.insert(task_id.clone(), (target.command().to_string(), root));
            }
        }

        // Channel for task completion events
        let (tx, mut rx) = mpsc::channel::<TaskEvent>(100);

        let mut task_results: Vec<TaskResult> = Vec::new();
        let mut success_count = 0usize;
        let mut failure_count = 0usize;
        let mut running_count = 0usize;
        let mut should_stop = false;

        // Main execution loop
        loop {
            // Spawn ready tasks up to concurrency limit
            if !should_stop {
                let ready: Vec<TaskId> = task_graph
                    .ready_tasks()
                    .iter()
                    .map(|t| (*t).clone())
                    .collect();

                for task_id in ready {
                    if running_count >= self.concurrency {
                        break;
                    }

                    // Mark as running
                    if task_graph.mark_running(&task_id).is_err() {
                        continue;
                    }

                    running_count += 1;

                    // Get command and working dir
                    let Some((command, cwd)) = task_commands.get(&task_id).cloned() else {
                        continue;
                    };

                    let tx = tx.clone();
                    let color = color_map
                        .get(&task_id.project().to_string())
                        .copied()
                        .unwrap_or(Color::White);

                    let prefix = format!("[{}]", task_id.project()).color(color).bold();
                    println!("{prefix} Starting {}", task_id.target());

                    // Spawn the task
                    tokio::spawn(async move {
                        let result = run_task(&task_id, &command, &cwd, color).await;
                        let _ = tx.send(TaskEvent::Completed { task_id, result }).await;
                    });
                }
            }

            // Exit if no tasks are running and we can't spawn more
            if running_count == 0 {
                break;
            }

            // Wait for a task to complete
            let Some(event) = rx.recv().await else {
                break;
            };

            match event {
                TaskEvent::Completed { task_id, result } => {
                    let color = color_map
                        .get(&task_id.project().to_string())
                        .copied()
                        .unwrap_or(Color::White);
                    let prefix = format!("[{}]", task_id.project()).color(color).bold();

                    if result.success {
                        println!(
                            "{prefix} {} {} in {:.2}s",
                            "✓".green(),
                            task_id.target(),
                            result.duration.as_secs_f64()
                        );
                        success_count += 1;
                    } else {
                        let exit_info = result
                            .exit_code
                            .map(|c| format!(" (exit code {c})"))
                            .unwrap_or_default();
                        eprintln!(
                            "{prefix} {} {} failed{exit_info} in {:.2}s",
                            "✗".red(),
                            task_id.target(),
                            result.duration.as_secs_f64()
                        );
                        failure_count += 1;

                        // In fail-fast mode, stop scheduling new tasks
                        if self.run_mode == RunMode::FailFast {
                            should_stop = true;
                        }
                    }

                    task_results.push(result);
                    running_count = running_count.saturating_sub(1);

                    // Mark complete in graph to unblock dependents
                    let _ = task_graph.mark_complete(&task_id);
                }
            }
        }

        let completed_count = success_count + failure_count;
        let skipped_count = total_tasks - completed_count;

        Ok(RunResult {
            success_count,
            failure_count,
            skipped_count,
            task_results,
            total_duration: start_time.elapsed(),
        })
    }
}

/// Run a single task and return the result.
async fn run_task(task_id: &TaskId, command: &str, cwd: &PathBuf, color: Color) -> TaskResult {
    let start = Instant::now();

    // Parse command - use shell to handle complex commands
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command).current_dir(cwd);

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let spawn_result = cmd.spawn();

    match spawn_result {
        Ok(mut child) => {
            // Stream stdout
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            let prefix = format!("[{}]", task_id.project()).color(color);

            let stdout_prefix = prefix.clone();
            let stdout_handle = tokio::spawn(async move {
                if let Some(stdout) = stdout {
                    let reader = BufReader::new(stdout);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        println!("{stdout_prefix} {line}");
                    }
                }
            });

            let stderr_prefix = prefix;
            let stderr_handle = tokio::spawn(async move {
                if let Some(stderr) = stderr {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        eprintln!("{stderr_prefix} {line}");
                    }
                }
            });

            // Wait for process
            let status = child.wait().await;

            // Wait for output streams to finish
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;

            let duration = start.elapsed();

            match status {
                Ok(status) => TaskResult {
                    task_id: task_id.clone(),
                    success: status.success(),
                    exit_code: status.code(),
                    duration,
                },
                Err(_) => TaskResult {
                    task_id: task_id.clone(),
                    success: false,
                    exit_code: None,
                    duration,
                },
            }
        }
        Err(_) => TaskResult {
            task_id: task_id.clone(),
            success: false,
            exit_code: None,
            duration: start.elapsed(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProjectConfig, TargetName};

    fn make_project(name: &str, deps: &[&str], targets: &[(&str, &str, &[&str])]) -> ProjectConfig {
        let deps_str = if deps.is_empty() {
            String::new()
        } else {
            let dep_list: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
            format!("depends_on = [{}]", dep_list.join(", "))
        };

        let targets_str: String = targets
            .iter()
            .map(|(target_name, cmd, target_deps)| {
                let target_deps_str = if target_deps.is_empty() {
                    String::new()
                } else {
                    let dep_list: Vec<String> =
                        target_deps.iter().map(|d| format!("\"{d}\"")).collect();
                    format!("depends_on = [{}]", dep_list.join(", "))
                };
                format!("[targets.{target_name}]\ncommand = \"{cmd}\"\n{target_deps_str}\n")
            })
            .collect();

        let toml = format!("[project]\nname = \"{name}\"\n{deps_str}\n\n{targets_str}");
        // Use /tmp as root since individual project directories don't actually exist
        ProjectConfig::from_str(&toml, PathBuf::from("/tmp")).unwrap()
    }

    fn tname(s: &str) -> TargetName {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn test_run_single_task() {
        let projects = vec![make_project("app", &[], &[("build", "echo hello", &[])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp"));
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        assert_eq!(result.success_count, 1);
        assert_eq!(result.failure_count, 0);
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn test_run_failing_task() {
        let projects = vec![make_project("app", &[], &[("build", "false", &[])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp"));
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        assert_eq!(result.success_count, 0);
        assert_eq!(result.failure_count, 1);
        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn test_dependency_ordering() {
        // lib must complete before app starts
        let projects = vec![
            make_project("app", &["lib"], &[("build", "echo app", &["^build"])]),
            make_project("lib", &[], &[("build", "echo lib", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp"));
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        assert_eq!(result.success_count, 2);
        assert_eq!(result.failure_count, 0);

        // Find the completion order
        let lib_idx = result
            .task_results
            .iter()
            .position(|r| r.task_id.project().as_str() == "lib")
            .unwrap();
        let app_idx = result
            .task_results
            .iter()
            .position(|r| r.task_id.project().as_str() == "app")
            .unwrap();

        // lib must complete before app
        assert!(lib_idx < app_idx, "lib should complete before app");
    }

    #[tokio::test]
    async fn test_parallel_independent_tasks() {
        // Three independent tasks should run in parallel
        let projects = vec![
            make_project("a", &[], &[("build", "sleep 0.1 && echo a", &[])]),
            make_project("b", &[], &[("build", "sleep 0.1 && echo b", &[])]),
            make_project("c", &[], &[("build", "sleep 0.1 && echo c", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp"));
        let start = Instant::now();
        let result = runner.run(task_graph, &project_graph).await.unwrap();
        let duration = start.elapsed();

        assert_eq!(result.success_count, 3);

        // If run in parallel, should take ~0.1s, not ~0.3s
        // Allow some slack for CI
        assert!(
            duration.as_secs_f64() < 0.25,
            "Tasks should run in parallel, took {:.2}s",
            duration.as_secs_f64()
        );
    }

    #[tokio::test]
    async fn test_fail_fast_mode() {
        // Task A fails, B depends on A so B should be skipped
        let projects = vec![
            make_project("a", &[], &[("build", "false", &[])]),
            make_project("b", &["a"], &[("build", "echo b", &["^build"])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp")).with_run_mode(RunMode::FailFast);
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        assert_eq!(result.failure_count, 1);
        // B depends on A, so B can't start (blocked by dependency)
        assert_eq!(result.skipped_count, 1);
    }

    #[tokio::test]
    async fn test_continue_mode() {
        // Even if one task fails, continue with others
        let projects = vec![
            make_project("a", &[], &[("build", "false", &[])]),
            make_project("b", &[], &[("build", "echo b", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp")).with_run_mode(RunMode::Continue);
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        // Both should have run
        assert_eq!(result.success_count, 1);
        assert_eq!(result.failure_count, 1);
    }

    #[tokio::test]
    async fn test_empty_graph() {
        let projects = vec![make_project("app", &[], &[("lint", "echo lint", &[])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp"));
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        assert_eq!(result.success_count, 0);
        assert_eq!(result.failure_count, 0);
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn test_concurrency_limit() {
        // 4 tasks with concurrency limit of 2
        let projects = vec![
            make_project("a", &[], &[("build", "sleep 0.05", &[])]),
            make_project("b", &[], &[("build", "sleep 0.05", &[])]),
            make_project("c", &[], &[("build", "sleep 0.05", &[])]),
            make_project("d", &[], &[("build", "sleep 0.05", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(2, PathBuf::from("/tmp"));
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        assert_eq!(result.success_count, 4);
    }

    #[tokio::test]
    async fn test_diamond_dependency() {
        // app depends on lib-a and lib-b, both depend on core
        // Execution order: core, then lib-a+lib-b in parallel, then app
        let projects = vec![
            make_project(
                "app",
                &["lib-a", "lib-b"],
                &[("build", "echo app", &["^build"])],
            ),
            make_project("lib-a", &["core"], &[("build", "echo lib-a", &["^build"])]),
            make_project("lib-b", &["core"], &[("build", "echo lib-b", &["^build"])]),
            make_project("core", &[], &[("build", "echo core", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let runner = TaskRunner::new(4, PathBuf::from("/tmp"));
        let result = runner.run(task_graph, &project_graph).await.unwrap();

        assert_eq!(result.success_count, 4);

        // Verify ordering: core must be first, app must be last
        let core_idx = result
            .task_results
            .iter()
            .position(|r| r.task_id.project().as_str() == "core")
            .unwrap();
        let app_idx = result
            .task_results
            .iter()
            .position(|r| r.task_id.project().as_str() == "app")
            .unwrap();

        assert_eq!(core_idx, 0, "core should complete first");
        assert_eq!(app_idx, 3, "app should complete last");
    }
}
