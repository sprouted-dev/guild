use std::fs;
use std::process::Command;

use guild_cli::run_affected;
use tempfile::tempdir;

/// Initialize a git repository with an initial commit and main branch.
fn init_git_repo(dir: &std::path::Path) {
    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("failed to init git repo");

    // Configure git user for commits
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .expect("failed to configure git email");

    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .expect("failed to configure git name");
}

fn git_add_all(dir: &std::path::Path) {
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .expect("failed to add files");
}

fn git_commit(dir: &std::path::Path, message: &str) {
    Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(dir)
        .output()
        .expect("failed to commit");
}

fn git_checkout_new_branch(dir: &std::path::Path, branch: &str) {
    Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(dir)
        .output()
        .expect("failed to create branch");
}

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

        // Create a src directory with a placeholder file
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();
    }
}

#[tokio::test]
async fn test_affected_with_changed_project() {
    let dir = tempdir().unwrap();

    // Initialize git repo
    init_git_repo(dir.path());

    // Create workspace with two independent projects
    create_workspace(
        dir.path(),
        &[
            ("app-a", "", &[("build", "echo a", &[])]),
            ("app-b", "", &[("build", "echo b", &[])]),
        ],
    );

    // Commit everything to main
    git_add_all(dir.path());
    git_commit(dir.path(), "Initial commit");

    // Create a feature branch
    git_checkout_new_branch(dir.path(), "feature");

    // Modify only app-a
    fs::write(
        dir.path().join("app-a").join("src").join("main.rs"),
        "fn main() { println!(\"hello\"); }",
    )
    .unwrap();
    git_add_all(dir.path());
    git_commit(dir.path(), "Modify app-a");

    // Run affected - should only run app-a
    let result = run_affected(dir.path(), "build", "main").await.unwrap();

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 0);
    assert!(result.is_success());

    // Verify only app-a was executed
    assert_eq!(result.task_results.len(), 1);
    assert_eq!(result.task_results[0].task_id.project().as_str(), "app-a");
}

#[tokio::test]
async fn test_affected_with_dependent_project() {
    let dir = tempdir().unwrap();

    // Initialize git repo
    init_git_repo(dir.path());

    // Create workspace where app depends on lib
    create_workspace(
        dir.path(),
        &[
            ("my-app", "my-lib", &[("build", "echo app", &["^build"])]),
            ("my-lib", "", &[("build", "echo lib", &[])]),
        ],
    );

    // Commit everything to main
    git_add_all(dir.path());
    git_commit(dir.path(), "Initial commit");

    // Create a feature branch
    git_checkout_new_branch(dir.path(), "feature");

    // Modify only my-lib
    fs::write(
        dir.path().join("my-lib").join("src").join("main.rs"),
        "pub fn lib_fn() {}",
    )
    .unwrap();
    git_add_all(dir.path());
    git_commit(dir.path(), "Modify my-lib");

    // Run affected - should run both my-lib (changed) and my-app (dependent)
    let result = run_affected(dir.path(), "build", "main").await.unwrap();

    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);
    assert!(result.is_success());

    // Verify both were executed
    let projects: Vec<&str> = result
        .task_results
        .iter()
        .map(|r| r.task_id.project().as_str())
        .collect();
    assert!(projects.contains(&"my-lib"));
    assert!(projects.contains(&"my-app"));
}

#[tokio::test]
async fn test_affected_no_changes() {
    let dir = tempdir().unwrap();

    // Initialize git repo
    init_git_repo(dir.path());

    // Create workspace
    create_workspace(dir.path(), &[("my-app", "", &[("build", "echo app", &[])])]);

    // Commit everything to main
    git_add_all(dir.path());
    git_commit(dir.path(), "Initial commit");

    // Create a feature branch (no changes)
    git_checkout_new_branch(dir.path(), "feature");

    // Run affected - should run nothing
    let result = run_affected(dir.path(), "build", "main").await.unwrap();

    assert_eq!(result.success_count, 0);
    assert_eq!(result.failure_count, 0);
    assert!(result.is_success());
}

#[tokio::test]
async fn test_affected_staged_changes() {
    let dir = tempdir().unwrap();

    // Initialize git repo
    init_git_repo(dir.path());

    // Create workspace with two independent projects
    create_workspace(
        dir.path(),
        &[
            ("app-a", "", &[("build", "echo a", &[])]),
            ("app-b", "", &[("build", "echo b", &[])]),
        ],
    );

    // Commit everything to main
    git_add_all(dir.path());
    git_commit(dir.path(), "Initial commit");

    // Create a feature branch
    git_checkout_new_branch(dir.path(), "feature");

    // Modify app-a and stage it (but don't commit)
    fs::write(
        dir.path().join("app-a").join("src").join("main.rs"),
        "fn main() { println!(\"staged\"); }",
    )
    .unwrap();
    git_add_all(dir.path());

    // Run affected - should detect staged changes
    let result = run_affected(dir.path(), "build", "main").await.unwrap();

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 0);
    assert_eq!(result.task_results[0].task_id.project().as_str(), "app-a");
}

#[tokio::test]
async fn test_affected_unstaged_changes() {
    let dir = tempdir().unwrap();

    // Initialize git repo
    init_git_repo(dir.path());

    // Create workspace with two independent projects
    create_workspace(
        dir.path(),
        &[
            ("app-a", "", &[("build", "echo a", &[])]),
            ("app-b", "", &[("build", "echo b", &[])]),
        ],
    );

    // Commit everything to main
    git_add_all(dir.path());
    git_commit(dir.path(), "Initial commit");

    // Create a feature branch
    git_checkout_new_branch(dir.path(), "feature");

    // Modify app-b but don't stage it
    fs::write(
        dir.path().join("app-b").join("src").join("main.rs"),
        "fn main() { println!(\"unstaged\"); }",
    )
    .unwrap();

    // Run affected - should detect unstaged changes
    let result = run_affected(dir.path(), "build", "main").await.unwrap();

    assert_eq!(result.success_count, 1);
    assert_eq!(result.failure_count, 0);
    assert_eq!(result.task_results[0].task_id.project().as_str(), "app-b");
}

#[tokio::test]
async fn test_affected_transitive_dependents() {
    let dir = tempdir().unwrap();

    // Initialize git repo
    init_git_repo(dir.path());

    // Create workspace: app -> lib -> core
    create_workspace(
        dir.path(),
        &[
            ("my-app", "my-lib", &[("build", "echo app", &["^build"])]),
            ("my-lib", "core", &[("build", "echo lib", &["^build"])]),
            ("core", "", &[("build", "echo core", &[])]),
        ],
    );

    // Commit everything to main
    git_add_all(dir.path());
    git_commit(dir.path(), "Initial commit");

    // Create a feature branch
    git_checkout_new_branch(dir.path(), "feature");

    // Modify only core
    fs::write(
        dir.path().join("core").join("src").join("main.rs"),
        "pub fn core_fn() {}",
    )
    .unwrap();
    git_add_all(dir.path());
    git_commit(dir.path(), "Modify core");

    // Run affected - should run all three (core changed, lib and app are transitive dependents)
    let result = run_affected(dir.path(), "build", "main").await.unwrap();

    assert_eq!(result.success_count, 3);
    assert_eq!(result.failure_count, 0);
    assert!(result.is_success());

    // Verify all were executed
    let projects: Vec<&str> = result
        .task_results
        .iter()
        .map(|r| r.task_id.project().as_str())
        .collect();
    assert!(projects.contains(&"core"));
    assert!(projects.contains(&"my-lib"));
    assert!(projects.contains(&"my-app"));
}
