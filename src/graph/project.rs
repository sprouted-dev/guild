use std::collections::{HashMap, HashSet};

use crate::config::{ProjectConfig, ProjectName};
use crate::error::GraphError;

/// A directed acyclic graph of project dependencies.
#[derive(Debug)]
pub struct ProjectGraph {
    /// Map of project name to its configuration.
    projects: HashMap<ProjectName, ProjectConfig>,
    /// Adjacency list: project -> set of projects it depends on.
    edges: HashMap<ProjectName, HashSet<ProjectName>>,
}

impl ProjectGraph {
    /// Build a project dependency graph from discovered projects.
    ///
    /// Validates that all dependency references are valid and that no cycles exist.
    pub fn build(projects: Vec<ProjectConfig>) -> Result<Self, GraphError> {
        let mut project_map: HashMap<ProjectName, ProjectConfig> = HashMap::new();
        let mut edges: HashMap<ProjectName, HashSet<ProjectName>> = HashMap::new();

        // Index all projects by name
        for project in projects {
            let name = project.name().clone();
            edges.insert(name.clone(), HashSet::new());
            project_map.insert(name, project);
        }

        // Build edges from depends_on declarations
        for (name, project) in &project_map {
            for dep in project.depends_on() {
                if !project_map.contains_key(dep) {
                    return Err(GraphError::UnknownProject {
                        name: dep.to_string(),
                        referenced_by: name.to_string(),
                    });
                }
                edges.get_mut(name).unwrap().insert(dep.clone());
            }
        }

        let graph = Self {
            projects: project_map,
            edges,
        };

        graph.check_cycles()?;

        Ok(graph)
    }

    /// Returns all projects in topological order (dependencies before dependents).
    pub fn topological_order(&self) -> Result<Vec<&ProjectName>, GraphError> {
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();
        let mut order = Vec::new();

        for name in self.projects.keys() {
            if !visited.contains(name) {
                self.topo_visit(name, &mut visited, &mut temp_visited, &mut order)?;
            }
        }

        Ok(order)
    }

    /// Get a project configuration by name.
    pub fn get(&self, name: &ProjectName) -> Option<&ProjectConfig> {
        self.projects.get(name)
    }

    /// Get all project names.
    pub fn project_names(&self) -> impl Iterator<Item = &ProjectName> {
        self.projects.keys()
    }

    /// Get the direct dependencies of a project.
    pub fn dependencies(&self, name: &ProjectName) -> Option<&HashSet<ProjectName>> {
        self.edges.get(name)
    }

    /// Get the number of projects in the graph.
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    /// Returns true if the graph has no projects.
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    fn check_cycles(&self) -> Result<(), GraphError> {
        // topological_order detects cycles via temp_visited
        self.topological_order().map(|_| ())
    }

    fn topo_visit<'a>(
        &'a self,
        name: &'a ProjectName,
        visited: &mut HashSet<ProjectName>,
        temp_visited: &mut HashSet<ProjectName>,
        order: &mut Vec<&'a ProjectName>,
    ) -> Result<(), GraphError> {
        if temp_visited.contains(name) {
            return Err(GraphError::CycleDetected {
                cycle: name.to_string(),
            });
        }
        if visited.contains(name) {
            return Ok(());
        }

        temp_visited.insert(name.clone());

        if let Some(deps) = self.edges.get(name) {
            for dep in deps {
                self.topo_visit(dep, visited, temp_visited, order)?;
            }
        }

        temp_visited.remove(name);
        visited.insert(name.clone());
        order.push(name);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;
    use std::path::PathBuf;

    fn make_project(name: &str, deps: &[&str]) -> ProjectConfig {
        let deps_str = if deps.is_empty() {
            String::new()
        } else {
            let dep_list: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
            format!("depends_on = [{}]", dep_list.join(", "))
        };
        let toml = format!(
            "[project]\nname = \"{name}\"\n{deps_str}\n\n[targets.build]\ncommand = \"echo build\"\n"
        );
        ProjectConfig::from_str(&toml, PathBuf::from(format!("/tmp/{name}"))).unwrap()
    }

    #[test]
    fn test_build_simple_graph() {
        let projects = vec![make_project("app", &["lib"]), make_project("lib", &[])];
        let graph = ProjectGraph::build(projects).unwrap();
        assert_eq!(graph.len(), 2);
    }

    #[test]
    fn test_topological_order() {
        let projects = vec![make_project("app", &["lib"]), make_project("lib", &[])];
        let graph = ProjectGraph::build(projects).unwrap();
        let order = graph.topological_order().unwrap();
        let names: Vec<&str> = order.iter().map(|n| n.as_str()).collect();
        let lib_idx = names.iter().position(|&n| n == "lib").unwrap();
        let app_idx = names.iter().position(|&n| n == "app").unwrap();
        assert!(lib_idx < app_idx);
    }

    #[test]
    fn test_cycle_detection() {
        let projects = vec![make_project("a", &["b"]), make_project("b", &["a"])];
        assert!(ProjectGraph::build(projects).is_err());
    }

    #[test]
    fn test_unknown_dependency() {
        let projects = vec![make_project("app", &["nonexistent"])];
        assert!(ProjectGraph::build(projects).is_err());
    }

    #[test]
    fn test_empty_graph() {
        let graph = ProjectGraph::build(vec![]).unwrap();
        assert!(graph.is_empty());
    }

    #[test]
    fn test_diamond_dependency() {
        let projects = vec![
            make_project("app", &["lib-a", "lib-b"]),
            make_project("lib-a", &["core"]),
            make_project("lib-b", &["core"]),
            make_project("core", &[]),
        ];
        let graph = ProjectGraph::build(projects).unwrap();
        let order = graph.topological_order().unwrap();
        let names: Vec<&str> = order.iter().map(|n| n.as_str()).collect();
        let core_idx = names.iter().position(|&n| n == "core").unwrap();
        let app_idx = names.iter().position(|&n| n == "app").unwrap();
        assert!(core_idx < app_idx);
    }
}
