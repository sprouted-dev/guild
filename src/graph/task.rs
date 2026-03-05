use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::config::{DependsOn, ProjectName, TargetName};
use crate::error::TaskGraphError;
use crate::graph::ProjectGraph;

/// A unique identifier for a task: (project, target) pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskId {
    project: ProjectName,
    target: TargetName,
}

impl TaskId {
    pub fn new(project: ProjectName, target: TargetName) -> Self {
        Self { project, target }
    }

    pub fn project(&self) -> &ProjectName {
        &self.project
    }

    pub fn target(&self) -> &TargetName {
        &self.target
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.project, self.target)
    }
}

/// The state of a task in the execution graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Task is waiting for dependencies to complete.
    Pending,
    /// Task is ready to execute (all deps complete).
    Ready,
    /// Task is currently executing.
    Running,
    /// Task completed successfully.
    Completed,
}

/// A fine-grained task dependency graph built from the project graph.
///
/// Expands project-level dependencies into target-level task dependencies,
/// resolving local and upstream (`^`) dependency references.
#[derive(Debug)]
pub struct TaskGraph {
    /// All tasks in the graph.
    tasks: HashSet<TaskId>,
    /// Dependencies: task -> set of tasks it depends on.
    dependencies: HashMap<TaskId, HashSet<TaskId>>,
    /// Reverse dependencies: task -> set of tasks that depend on it.
    dependents: HashMap<TaskId, HashSet<TaskId>>,
    /// Current state of each task.
    states: HashMap<TaskId, TaskState>,
    /// Count of incomplete dependencies for each task.
    pending_deps: HashMap<TaskId, usize>,
}

impl TaskGraph {
    /// Build a task graph from a project graph for the given target.
    ///
    /// This expands the project-level DAG into a fine-grained task-level DAG,
    /// resolving all `depends_on` references in each target configuration.
    pub fn build(
        project_graph: &ProjectGraph,
        target: &TargetName,
    ) -> Result<Self, TaskGraphError> {
        let mut tasks = HashSet::new();
        let mut dependencies: HashMap<TaskId, HashSet<TaskId>> = HashMap::new();
        let mut dependents: HashMap<TaskId, HashSet<TaskId>> = HashMap::new();

        // First pass: collect all tasks that have this target
        for project_name in project_graph.project_names() {
            if let Some(project) = project_graph.get(project_name)
                && project.targets().contains_key(target)
            {
                let task_id = TaskId::new(project_name.clone(), target.clone());
                tasks.insert(task_id.clone());
                dependencies.insert(task_id.clone(), HashSet::new());
                dependents.insert(task_id, HashSet::new());
            }
        }

        let mut graph = Self {
            tasks,
            dependencies,
            dependents,
            states: HashMap::new(),
            pending_deps: HashMap::new(),
        };

        // Resolve all dependencies using worklist approach
        graph.resolve_transitive_deps(project_graph)?;
        graph.check_cycles()?;
        graph.initialize_state();

        Ok(graph)
    }

    /// Recursively resolve dependencies for all tasks in the graph.
    fn resolve_transitive_deps(
        &mut self,
        project_graph: &ProjectGraph,
    ) -> Result<(), TaskGraphError> {
        let mut to_process: Vec<TaskId> = self.tasks.iter().cloned().collect();
        let mut processed: HashSet<TaskId> = HashSet::new();

        while let Some(task_id) = to_process.pop() {
            if processed.contains(&task_id) {
                continue;
            }
            processed.insert(task_id.clone());

            let project = match project_graph.get(task_id.project()) {
                Some(p) => p,
                None => continue,
            };

            let target_config = match project.targets().get(task_id.target()) {
                Some(t) => t,
                None => continue,
            };

            for dep in target_config.depends_on() {
                match dep {
                    DependsOn::Local(dep_target) => {
                        let dep_task = TaskId::new(task_id.project().clone(), dep_target.clone());

                        if !project.targets().contains_key(dep_target) {
                            return Err(TaskGraphError::UnknownTarget {
                                target: dep_target.to_string(),
                                project: task_id.project().to_string(),
                                referencing_target: task_id.target().to_string(),
                            });
                        }

                        if !self.tasks.contains(&dep_task) {
                            self.tasks.insert(dep_task.clone());
                            self.dependencies.insert(dep_task.clone(), HashSet::new());
                            self.dependents.insert(dep_task.clone(), HashSet::new());
                            to_process.push(dep_task.clone());
                        }

                        self.dependencies
                            .get_mut(&task_id)
                            .unwrap()
                            .insert(dep_task.clone());
                        self.dependents
                            .get_mut(&dep_task)
                            .unwrap()
                            .insert(task_id.clone());
                    }
                    DependsOn::Upstream(dep_target) => {
                        if let Some(project_deps) = project_graph.dependencies(task_id.project()) {
                            for dep_project in project_deps {
                                let dep_task = TaskId::new(dep_project.clone(), dep_target.clone());

                                if let Some(dep_proj_config) = project_graph.get(dep_project)
                                    && dep_proj_config.targets().contains_key(dep_target)
                                {
                                    if !self.tasks.contains(&dep_task) {
                                        self.tasks.insert(dep_task.clone());
                                        self.dependencies.insert(dep_task.clone(), HashSet::new());
                                        self.dependents.insert(dep_task.clone(), HashSet::new());
                                        to_process.push(dep_task.clone());
                                    }

                                    self.dependencies
                                        .get_mut(&task_id)
                                        .unwrap()
                                        .insert(dep_task.clone());
                                    self.dependents
                                        .get_mut(&dep_task)
                                        .unwrap()
                                        .insert(task_id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Check for cycles in the task graph.
    fn check_cycles(&self) -> Result<(), TaskGraphError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for task in &self.tasks {
            if !visited.contains(task) {
                self.detect_cycle_dfs(task, &mut visited, &mut rec_stack)?;
            }
        }

        Ok(())
    }

    fn detect_cycle_dfs(
        &self,
        task: &TaskId,
        visited: &mut HashSet<TaskId>,
        rec_stack: &mut HashSet<TaskId>,
    ) -> Result<(), TaskGraphError> {
        visited.insert(task.clone());
        rec_stack.insert(task.clone());

        if let Some(deps) = self.dependencies.get(task) {
            for dep in deps {
                if !visited.contains(dep) {
                    self.detect_cycle_dfs(dep, visited, rec_stack)?;
                } else if rec_stack.contains(dep) {
                    return Err(TaskGraphError::CycleDetected {
                        project: dep.project().to_string(),
                        target: dep.target().to_string(),
                    });
                }
            }
        }

        rec_stack.remove(task);
        Ok(())
    }

    /// Initialize task states: tasks with no dependencies are Ready, others are Pending.
    fn initialize_state(&mut self) {
        for task in &self.tasks {
            let dep_count = self.dependencies.get(task).map_or(0, |d| d.len());
            self.pending_deps.insert(task.clone(), dep_count);
            if dep_count == 0 {
                self.states.insert(task.clone(), TaskState::Ready);
            } else {
                self.states.insert(task.clone(), TaskState::Pending);
            }
        }
    }

    /// Get all tasks that are ready to execute.
    pub fn ready_tasks(&self) -> Vec<&TaskId> {
        self.states
            .iter()
            .filter(|(_, state)| **state == TaskState::Ready)
            .map(|(task, _)| task)
            .collect()
    }

    /// Mark a task as complete and return the tasks that became ready.
    pub fn mark_complete(&mut self, task_id: &TaskId) -> Result<Vec<TaskId>, TaskGraphError> {
        if !self.tasks.contains(task_id) {
            return Err(TaskGraphError::TaskNotFound {
                project: task_id.project().to_string(),
                target: task_id.target().to_string(),
            });
        }

        self.states.insert(task_id.clone(), TaskState::Completed);

        let mut newly_ready = Vec::new();

        if let Some(dependents) = self.dependents.get(task_id).cloned() {
            for dependent in dependents {
                if let Some(count) = self.pending_deps.get_mut(&dependent) {
                    *count = count.saturating_sub(1);
                    if *count == 0
                        && let Some(state) = self.states.get(&dependent)
                        && *state == TaskState::Pending
                    {
                        self.states.insert(dependent.clone(), TaskState::Ready);
                        newly_ready.push(dependent);
                    }
                }
            }
        }

        Ok(newly_ready)
    }

    /// Mark a task as running.
    pub fn mark_running(&mut self, task_id: &TaskId) -> Result<(), TaskGraphError> {
        if !self.tasks.contains(task_id) {
            return Err(TaskGraphError::TaskNotFound {
                project: task_id.project().to_string(),
                target: task_id.target().to_string(),
            });
        }
        self.states.insert(task_id.clone(), TaskState::Running);
        Ok(())
    }

    /// Get the state of a task.
    pub fn state(&self, task_id: &TaskId) -> Option<TaskState> {
        self.states.get(task_id).copied()
    }

    /// Get all tasks in the graph.
    pub fn tasks(&self) -> impl Iterator<Item = &TaskId> {
        self.tasks.iter()
    }

    /// Get the direct dependencies of a task.
    pub fn dependencies_of(&self, task_id: &TaskId) -> Option<&HashSet<TaskId>> {
        self.dependencies.get(task_id)
    }

    /// Get the number of tasks in the graph.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Returns true if the graph has no tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Check if all tasks are completed.
    pub fn all_completed(&self) -> bool {
        self.states.values().all(|s| *s == TaskState::Completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;
    use std::path::PathBuf;

    fn make_project(name: &str, deps: &[&str], targets: &[(&str, &[&str])]) -> ProjectConfig {
        let deps_str = if deps.is_empty() {
            String::new()
        } else {
            let dep_list: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
            format!("depends_on = [{}]", dep_list.join(", "))
        };

        let targets_str: String = targets
            .iter()
            .map(|(target_name, target_deps)| {
                let target_deps_str = if target_deps.is_empty() {
                    String::new()
                } else {
                    let dep_list: Vec<String> =
                        target_deps.iter().map(|d| format!("\"{d}\"")).collect();
                    format!("depends_on = [{}]", dep_list.join(", "))
                };
                format!(
                    "[targets.{target_name}]\ncommand = \"echo {target_name}\"\n{target_deps_str}\n"
                )
            })
            .collect();

        let toml = format!("[project]\nname = \"{name}\"\n{deps_str}\n\n{targets_str}");
        ProjectConfig::from_str(&toml, PathBuf::from(format!("/tmp/{name}"))).unwrap()
    }

    fn tname(s: &str) -> TargetName {
        s.parse().unwrap()
    }

    fn pname(s: &str) -> ProjectName {
        s.parse().unwrap()
    }

    #[test]
    fn test_build_simple_task_graph() {
        let projects = vec![
            make_project("app", &[], &[("build", &[])]),
            make_project("lib", &[], &[("build", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        assert_eq!(task_graph.len(), 2);
    }

    #[test]
    fn test_local_dependency_resolution() {
        // test depends on build locally
        let projects = vec![make_project(
            "app",
            &[],
            &[("build", &[]), ("test", &["build"])],
        )];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("test")).unwrap();

        assert_eq!(task_graph.len(), 2);

        let test_task = TaskId::new(pname("app"), tname("test"));
        let build_task = TaskId::new(pname("app"), tname("build"));

        let deps = task_graph.dependencies_of(&test_task).unwrap();
        assert!(deps.contains(&build_task));
    }

    #[test]
    fn test_upstream_dependency_resolution() {
        // app depends on lib at project level
        // app:build depends on ^build (upstream)
        let projects = vec![
            make_project("app", &["lib"], &[("build", &["^build"])]),
            make_project("lib", &[], &[("build", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        assert_eq!(task_graph.len(), 2);

        let app_build = TaskId::new(pname("app"), tname("build"));
        let lib_build = TaskId::new(pname("lib"), tname("build"));

        let deps = task_graph.dependencies_of(&app_build).unwrap();
        assert!(deps.contains(&lib_build));
    }

    #[test]
    fn test_upstream_fans_out_to_all_deps() {
        // app depends on lib-a and lib-b
        // app:build depends on ^build
        let projects = vec![
            make_project("app", &["lib-a", "lib-b"], &[("build", &["^build"])]),
            make_project("lib-a", &[], &[("build", &[])]),
            make_project("lib-b", &[], &[("build", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        assert_eq!(task_graph.len(), 3);

        let app_build = TaskId::new(pname("app"), tname("build"));
        let lib_a_build = TaskId::new(pname("lib-a"), tname("build"));
        let lib_b_build = TaskId::new(pname("lib-b"), tname("build"));

        let deps = task_graph.dependencies_of(&app_build).unwrap();
        assert!(deps.contains(&lib_a_build));
        assert!(deps.contains(&lib_b_build));
    }

    #[test]
    fn test_ready_tasks_initial() {
        let projects = vec![
            make_project("app", &["lib"], &[("build", &["^build"])]),
            make_project("lib", &[], &[("build", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let ready = task_graph.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].project().as_str(), "lib");
        assert_eq!(ready[0].target().as_str(), "build");
    }

    #[test]
    fn test_mark_complete_unblocks_dependents() {
        let projects = vec![
            make_project("app", &["lib"], &[("build", &["^build"])]),
            make_project("lib", &[], &[("build", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let mut task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let lib_build = TaskId::new(pname("lib"), tname("build"));
        let app_build = TaskId::new(pname("app"), tname("build"));

        // lib:build should be ready initially
        assert_eq!(task_graph.state(&lib_build), Some(TaskState::Ready));
        assert_eq!(task_graph.state(&app_build), Some(TaskState::Pending));

        // Mark lib:build complete
        let newly_ready = task_graph.mark_complete(&lib_build).unwrap();

        assert_eq!(newly_ready.len(), 1);
        assert_eq!(newly_ready[0], app_build);
        assert_eq!(task_graph.state(&app_build), Some(TaskState::Ready));
    }

    #[test]
    fn test_cycle_detection_at_target_level() {
        // Create a cycle at the target level: build -> test -> build
        let projects = vec![make_project(
            "app",
            &[],
            &[("build", &["test"]), ("test", &["build"])],
        )];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let result = TaskGraph::build(&project_graph, &tname("build"));

        assert!(result.is_err());
        match result.unwrap_err() {
            TaskGraphError::CycleDetected { .. } => {}
            e => panic!("Expected CycleDetected error, got {:?}", e),
        }
    }

    #[test]
    fn test_unknown_local_target_error() {
        // Reference a target that doesn't exist
        let projects = vec![make_project("app", &[], &[("build", &["nonexistent"])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let result = TaskGraph::build(&project_graph, &tname("build"));

        assert!(result.is_err());
        match result.unwrap_err() {
            TaskGraphError::UnknownTarget { target, .. } => {
                assert_eq!(target, "nonexistent");
            }
            e => panic!("Expected UnknownTarget error, got {:?}", e),
        }
    }

    #[test]
    fn test_empty_graph_when_no_projects_have_target() {
        let projects = vec![make_project("app", &[], &[("lint", &[])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        assert!(task_graph.is_empty());
    }

    #[test]
    fn test_all_completed() {
        let projects = vec![make_project("app", &[], &[("build", &[])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let mut task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        assert!(!task_graph.all_completed());

        let app_build = TaskId::new(pname("app"), tname("build"));
        task_graph.mark_complete(&app_build).unwrap();

        assert!(task_graph.all_completed());
    }

    #[test]
    fn test_mark_running() {
        let projects = vec![make_project("app", &[], &[("build", &[])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let mut task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let app_build = TaskId::new(pname("app"), tname("build"));
        task_graph.mark_running(&app_build).unwrap();

        assert_eq!(task_graph.state(&app_build), Some(TaskState::Running));
    }

    #[test]
    fn test_task_not_found_error() {
        let projects = vec![make_project("app", &[], &[("build", &[])])];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let mut task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        let nonexistent = TaskId::new(pname("app"), tname("nonexistent"));
        let result = task_graph.mark_complete(&nonexistent);

        assert!(result.is_err());
        match result.unwrap_err() {
            TaskGraphError::TaskNotFound { .. } => {}
            e => panic!("Expected TaskNotFound error, got {:?}", e),
        }
    }

    #[test]
    fn test_diamond_dependency() {
        // app -> lib-a -> core
        // app -> lib-b -> core
        // All have build target with ^build deps
        let projects = vec![
            make_project("app", &["lib-a", "lib-b"], &[("build", &["^build"])]),
            make_project("lib-a", &["core"], &[("build", &["^build"])]),
            make_project("lib-b", &["core"], &[("build", &["^build"])]),
            make_project("core", &[], &[("build", &[])]),
        ];
        let project_graph = ProjectGraph::build(projects).unwrap();
        let mut task_graph = TaskGraph::build(&project_graph, &tname("build")).unwrap();

        assert_eq!(task_graph.len(), 4);

        // Only core:build should be ready initially
        let ready = task_graph.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].project().as_str(), "core");

        // Mark core complete, lib-a and lib-b should become ready
        let core_build = TaskId::new(pname("core"), tname("build"));
        let newly_ready = task_graph.mark_complete(&core_build).unwrap();
        assert_eq!(newly_ready.len(), 2);

        // Mark lib-a and lib-b complete, app should become ready
        let lib_a_build = TaskId::new(pname("lib-a"), tname("build"));
        let lib_b_build = TaskId::new(pname("lib-b"), tname("build"));
        task_graph.mark_complete(&lib_a_build).unwrap();
        let newly_ready = task_graph.mark_complete(&lib_b_build).unwrap();

        assert_eq!(newly_ready.len(), 1);
        assert_eq!(newly_ready[0].project().as_str(), "app");
    }

    #[test]
    fn test_task_id_display() {
        let task = TaskId::new(pname("my-app"), tname("build"));
        assert_eq!(task.to_string(), "my-app:build");
    }
}
