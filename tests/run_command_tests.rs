use std::fs;

use guild_cli::run_target;
use tempfile::tempdir;

#[allow(clippy::type_complexity)]
fn create_workspace(dir: &std::path::Path, projects: &[(&str, &str, &[(&str, &str, &[&str])])]) {
    // Create workspace guild.toml
    let workspace_toml = r#"[workspace]
name = "test-workspace"
projects = ["*"]
"#;
    fs::write(dir.join("guild.toml"), workspace_toml).unwrap();

    // Create project directories and guild.toml files
    for (name, deps, targets) in projects {
        let project_dir = dir.join(name);
        fs::create_dir_all(&project_dir).unwrap();

        let deps_str = if deps.is_empty() {
            String::new()
        } else {
            let dep_list: Vec<String> = deps.split(',').map(|d| format!("\"{d}\"")).collect();
            format!("depends_on = [{}]\n", dep_list.join(", "))
        };

        let targets_str: String = targets
            .iter()
            .map(|(target_name, cmd, target_deps)| {
                let target_deps_str = if target_deps.is_empty() {
                    String::new()
                } else {
                    let dep_list: Vec<String> =
                        target_deps.iter().map(|d| format!("\"{d}\"")).collect();
                    format!("depends_on = [{}]\n", dep_list.join(", "))
                };
                format!("[targets.{target_name}]\ncommand = \"{cmd}\"\n{target_deps_str}\n")
            })
            .collect();

        let project_toml = format!("[project]\nname = \"{name}\"\n{deps_str}\n{targets_str}");

        fs::write(project_dir.join("guild.toml"), project_toml).unwrap();
    }
}

#[tokio::test]
async fn test_run_single_project() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[("my-app", "", &[("build", "echo building", &[])])],
    );

    let result = run_target(dir.path(), "build", None).await.unwrap();

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 0);
    assert!(result.is_success());
}

#[tokio::test]
async fn test_run_multiple_projects() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[
            ("app-a", "", &[("build", "echo a", &[])]),
            ("app-b", "", &[("build", "echo b", &[])]),
            ("lib", "", &[("build", "echo lib", &[])]),
        ],
    );

    let result = run_target(dir.path(), "build", None).await.unwrap();

    assert_eq!(result.success_count, 3);
    assert_eq!(result.failure_count, 0);
    assert!(result.is_success());
}

#[tokio::test]
async fn test_run_with_dependencies() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[
            ("my-app", "my-lib", &[("build", "echo app", &["^build"])]),
            ("my-lib", "", &[("build", "echo lib", &[])]),
        ],
    );

    let result = run_target(dir.path(), "build", None).await.unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);

    // Verify lib completed before app
    let lib_idx = result
        .task_results
        .iter()
        .position(|r| r.task_id.project().as_str() == "my-lib")
        .unwrap();
    let app_idx = result
        .task_results
        .iter()
        .position(|r| r.task_id.project().as_str() == "my-app")
        .unwrap();
    assert!(lib_idx < app_idx, "lib should complete before app");
}

#[tokio::test]
async fn test_run_scoped_to_project() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[
            ("app-a", "", &[("build", "echo a", &[])]),
            ("app-b", "", &[("build", "echo b", &[])]),
        ],
    );

    // Run build only for app-a
    let result = run_target(dir.path(), "build", Some("app-a"))
        .await
        .unwrap();

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 0);
    assert_eq!(result.task_results[0].task_id.project().as_str(), "app-a");
}

#[tokio::test]
async fn test_run_scoped_includes_upstream_deps() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[
            ("my-app", "my-lib", &[("build", "echo app", &["^build"])]),
            ("my-lib", "", &[("build", "echo lib", &[])]),
            ("other", "", &[("build", "echo other", &[])]),
        ],
    );

    // Run build for my-app should include my-lib (its dependency) but not other
    let result = run_target(dir.path(), "build", Some("my-app"))
        .await
        .unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);

    // Verify only my-app and my-lib were executed, not other
    let projects: Vec<&str> = result
        .task_results
        .iter()
        .map(|r| r.task_id.project().as_str())
        .collect();
    assert!(projects.contains(&"my-app"));
    assert!(projects.contains(&"my-lib"));
    assert!(!projects.contains(&"other"));
}

#[tokio::test]
async fn test_run_failing_task() {
    let dir = tempdir().unwrap();
    create_workspace(dir.path(), &[("bad-app", "", &[("build", "exit 1", &[])])]);

    let result = run_target(dir.path(), "build", None).await.unwrap();

    assert_eq!(result.success_count, 0);
    assert_eq!(result.failure_count, 1);
    assert!(!result.is_success());
}

#[tokio::test]
async fn test_run_nonexistent_target() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[("my-app", "", &[("build", "echo build", &[])])],
    );

    // Run a target that doesn't exist
    let result = run_target(dir.path(), "test", None).await.unwrap();

    // Should succeed with 0 tasks (no projects have 'test' target)
    assert_eq!(result.success_count, 0);
    assert_eq!(result.failure_count, 0);
    assert!(result.is_success());
}

#[tokio::test]
async fn test_run_nonexistent_project() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[("my-app", "", &[("build", "echo build", &[])])],
    );

    // Run build for a project that doesn't exist
    let result = run_target(dir.path(), "build", Some("nonexistent")).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_run_from_subdirectory() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[("my-app", "", &[("build", "echo build", &[])])],
    );

    // Run from within my-app directory
    let subdir = dir.path().join("my-app");
    let result = run_target(&subdir, "build", None).await.unwrap();

    assert_eq!(result.success_count, 1);
    assert!(result.is_success());
}

#[tokio::test]
async fn test_run_diamond_dependency() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[
            ("app", "lib-a,lib-b", &[("build", "echo app", &["^build"])]),
            ("lib-a", "core", &[("build", "echo lib-a", &["^build"])]),
            ("lib-b", "core", &[("build", "echo lib-b", &["^build"])]),
            ("core", "", &[("build", "echo core", &[])]),
        ],
    );

    let result = run_target(dir.path(), "build", None).await.unwrap();

    assert_eq!(result.success_count, 4);
    assert_eq!(result.failure_count, 0);

    // Verify core completed first and app completed last
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

#[tokio::test]
async fn test_run_with_local_target_dependencies() {
    let dir = tempdir().unwrap();
    create_workspace(
        dir.path(),
        &[(
            "my-app",
            "",
            &[
                ("build", "echo build", &[]),
                ("test", "echo test", &["build"]),
            ],
        )],
    );

    let result = run_target(dir.path(), "test", None).await.unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);

    // Verify build completed before test
    let build_idx = result
        .task_results
        .iter()
        .position(|r| r.task_id.target().as_str() == "build")
        .unwrap();
    let test_idx = result
        .task_results
        .iter()
        .position(|r| r.task_id.target().as_str() == "test")
        .unwrap();
    assert!(build_idx < test_idx, "build should complete before test");
}
