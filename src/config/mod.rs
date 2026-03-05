mod project;
mod types;
mod workspace;

pub use project::{ProjectConfig, TargetConfig};
pub use types::{DependsOn, ProjectName, TargetName};
pub use workspace::WorkspaceConfig;
