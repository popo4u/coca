use std::path::{Path, PathBuf};

use crate::model::{ProviderKind, Session};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeTarget {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchMode {
    Resume,
    Fork,
}

impl LaunchMode {
    pub fn label(self) -> &'static str {
        match self {
            LaunchMode::Resume => "Execute",
            LaunchMode::Fork => "Fork",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchOptionKind {
    UseCurrentDir,
    Yolo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchOption {
    pub kind: LaunchOptionKind,
    pub label: String,
    pub enabled: bool,
}

pub fn default_resume_target(session: &Session) -> ResumeTarget {
    build_launch_target(session, LaunchMode::Resume, &PathBuf::new(), &[])
}

pub fn launch_options(session: &Session, current_cwd: &Path) -> Vec<LaunchOption> {
    let current = current_cwd.to_string_lossy();
    let mut options = vec![LaunchOption {
        kind: LaunchOptionKind::UseCurrentDir,
        label: format!("Use current directory instead of session cwd ({current})"),
        enabled: false,
    }];
    let yolo_label = match session.provider {
        ProviderKind::Codex => "YOLO mode (--dangerously-bypass-approvals-and-sandbox)",
        ProviderKind::Claude => "Skip permissions (--dangerously-skip-permissions)",
    };
    options.push(LaunchOption {
        kind: LaunchOptionKind::Yolo,
        label: yolo_label.to_string(),
        enabled: false,
    });
    options
}

pub fn build_launch_target(
    session: &Session,
    mode: LaunchMode,
    current_cwd: &Path,
    options: &[LaunchOption],
) -> ResumeTarget {
    let use_current_dir = option_enabled(options, LaunchOptionKind::UseCurrentDir);
    let yolo = option_enabled(options, LaunchOptionKind::Yolo);
    let cwd = if use_current_dir {
        Some(current_cwd.to_path_buf())
    } else {
        resume_cwd(session)
    };

    match session.provider {
        ProviderKind::Codex => {
            let mut args = vec![match mode {
                LaunchMode::Resume => "resume".to_string(),
                LaunchMode::Fork => "fork".to_string(),
            }];
            if use_current_dir {
                args.push("-C".to_string());
                args.push(current_cwd.to_string_lossy().to_string());
            }
            if yolo {
                args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
            }
            args.push(session.id.clone());
            ResumeTarget {
                program: "codex".to_string(),
                args,
                cwd,
            }
        }
        ProviderKind::Claude => {
            let mut args = vec!["--resume".to_string(), session.id.clone()];
            if mode == LaunchMode::Fork {
                args.push("--fork-session".to_string());
            }
            if yolo {
                args.push("--dangerously-skip-permissions".to_string());
            }
            ResumeTarget {
                program: "claude".to_string(),
                args,
                cwd,
            }
        }
    }
}

pub fn option_enabled(options: &[LaunchOption], kind: LaunchOptionKind) -> bool {
    options
        .iter()
        .find(|option| option.kind == kind)
        .map(|option| option.enabled)
        .unwrap_or(false)
}

fn resume_cwd(session: &Session) -> Option<PathBuf> {
    let cwd = session.cwd.trim();
    if cwd.is_empty() {
        None
    } else {
        Some(PathBuf::from(cwd))
    }
}
