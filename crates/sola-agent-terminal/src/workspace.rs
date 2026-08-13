//! In-memory project / workspace ids for the skeleton.
//!
//! Persist and spawn land in a later slice. One hardcoded project is enough
//! to prove the rail + pane composition.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub id: String,
    #[allow(dead_code)]
    pub project_id: String,
    pub name: String,
    pub path: PathBuf,
}

/// Skeleton seed: one project named after this checkout's folder, one
/// workspace on `cwd` (the main tree until spawn exists).
pub fn seed() -> (Project, Workspace) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Sola")
        .to_string();
    let project = Project {
        id: "proj-seed".into(),
        name: name.clone(),
    };
    let workspace = Workspace {
        id: "ws-main".into(),
        project_id: project.id.clone(),
        name: "main".into(),
        path: cwd,
    };
    (project, workspace)
}
