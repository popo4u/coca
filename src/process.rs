use anyhow::Result;
use coca_core::launch::ResumeTarget;

#[cfg(unix)]
pub fn exec_resume(target: ResumeTarget) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut command = std::process::Command::new(&target.program);
    command.args(&target.args);
    if let Some(cwd) = &target.cwd {
        command.current_dir(cwd);
    }
    Err(command.exec().into())
}

#[cfg(not(unix))]
pub fn exec_resume(target: ResumeTarget) -> Result<()> {
    let mut command = std::process::Command::new(&target.program);
    command.args(&target.args);
    if let Some(cwd) = &target.cwd {
        command.current_dir(cwd);
    }

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("{} exited with status {}", target.program, status);
    }
}
