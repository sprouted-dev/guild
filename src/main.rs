mod cli;

use anyhow::Result;
use clap::Parser;

use cli::{CacheCommand, Cli, Commands};
use guild_cli::{
    ProjectGraph, WorkspaceConfig, discover_projects, find_workspace_root, print_error,
    print_header, print_not_implemented, print_project_entry, print_success, run_init, run_target,
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        print_error(&format!("{e}"));
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
        }

        Some(Commands::List) => {
            let cwd = std::env::current_dir()?;
            let root = find_workspace_root(&cwd)?;
            let workspace = WorkspaceConfig::from_file(&root.join("guild.toml"))?;
            let projects = discover_projects(&workspace)?;

            print_header(&format!("Workspace: {}", workspace.name()));
            println!("  {} projects discovered\n", projects.len());

            for project in &projects {
                print_project_entry(
                    project.name().as_str(),
                    &project.root().display().to_string(),
                    project.tags(),
                );
            }
        }

        Some(Commands::Graph) => {
            let cwd = std::env::current_dir()?;
            let root = find_workspace_root(&cwd)?;
            let workspace = WorkspaceConfig::from_file(&root.join("guild.toml"))?;
            let projects = discover_projects(&workspace)?;
            let graph = ProjectGraph::build(projects)?;

            print_header("Project Dependency Graph");
            let order = graph.topological_order()?;
            for name in &order {
                let deps = graph.dependencies(name).unwrap();
                if deps.is_empty() {
                    println!("  {name}");
                } else {
                    let dep_names: Vec<String> = deps.iter().map(|d| d.to_string()).collect();
                    println!("  {name} -> {}", dep_names.join(", "));
                }
            }
        }

        Some(Commands::Dev) => print_not_implemented("dev"),
        Some(Commands::Build) => print_not_implemented("build"),
        Some(Commands::Test) => print_not_implemented("test"),
        Some(Commands::Lint) => print_not_implemented("lint"),
        Some(Commands::Run { target, project }) => {
            let cwd = std::env::current_dir()?;
            let result = run_target(&cwd, &target, project.as_deref()).await?;
            if !result.is_success() {
                std::process::exit(1);
            }
        }
        Some(Commands::Affected { target }) => print_not_implemented(&format!("affected {target}")),
        Some(Commands::Cache { command }) => match command {
            CacheCommand::Status => print_not_implemented("cache status"),
            CacheCommand::Clean => print_not_implemented("cache clean"),
        },
        Some(Commands::Init { yes }) => {
            let cwd = std::env::current_dir()?;
            // Use the current directory name as the workspace name
            let workspace_name = cwd
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "workspace".to_string());

            print_header(&format!("Initializing Guild workspace: {workspace_name}"));

            let result = run_init(&cwd, &workspace_name, yes)?;

            println!();
            if result.written.is_empty() && result.skipped.is_empty() {
                print_success(
                    "No projects detected. Create project manifests first (package.json, Cargo.toml, go.mod, or pyproject.toml).",
                );
            } else {
                print_success(&format!(
                    "Initialized {} guild.toml file(s), skipped {} existing",
                    result.written.len(),
                    result.skipped.len()
                ));
            }
        }
    }

    Ok(())
}
