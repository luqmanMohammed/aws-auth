use aws_config::Region;
use aws_sdk_sso::config::Credentials;
use std::collections::HashMap;
use std::io;
use std::process::{Command, ExitStatus, Stdio};

pub struct ExecExecInputs {
    pub region: Region,
    pub arguments: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),
    #[error("Failed to start program: {0}")]
    ProgramSpawnFailed(io::Error),
    #[error("Failed to wait for program: {0}")]
    ProgramWaitFailed(io::Error),
}

pub type Result = std::result::Result<(), Error>;

#[cfg(unix)]
fn child_exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    // Shells report a signalled child as 128 + signal number; `code()` is None for those.
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

#[cfg(not(unix))]
fn child_exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

pub async fn exec_exec(credentials: Credentials, exec_inputs: ExecExecInputs) -> Result {
    let program = exec_inputs
        .arguments
        .first()
        .ok_or(Error::InvalidCommand("Missing Program".to_string()))?;
    let args = &(exec_inputs.arguments)[1..];

    let mut envs = HashMap::new();

    envs.insert("AWS_REGION", exec_inputs.region.as_ref());
    envs.insert("AWS_DEFAULT_REGION", exec_inputs.region.as_ref());
    envs.insert("AWS_ACCESS_KEY_ID", credentials.access_key_id());
    envs.insert("AWS_SECRET_ACCESS_KEY", credentials.secret_access_key());
    if let Some(session_token) = credentials.session_token() {
        envs.insert("AWS_SESSION_TOKEN", session_token);
    }

    let mut child = Command::new(program)
        .args(args)
        .envs(envs)
        .stdin(Stdio::inherit())
        .stderr(io::stderr())
        .stdout(io::stdout())
        .spawn()
        .map_err(Error::ProgramSpawnFailed)?;

    let status = child.wait().map_err(Error::ProgramWaitFailed)?;

    std::process::exit(child_exit_code(status))
}

// Written by an AI assistant and not human reviewed.
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    // A wait status packs a normal exit into the high byte and a signal into the low bits.
    fn exited(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    fn signalled(signal: i32) -> ExitStatus {
        ExitStatus::from_raw(signal)
    }

    #[test]
    fn a_normal_exit_is_reported_verbatim() {
        for code in [0, 1, 42, 127, 255] {
            assert_eq!(child_exit_code(exited(code)), code);
        }
    }

    #[test]
    fn a_signalled_child_is_reported_the_way_a_shell_does() {
        assert_eq!(child_exit_code(signalled(2)), 130, "SIGINT");
        assert_eq!(child_exit_code(signalled(9)), 137, "SIGKILL");
        assert_eq!(child_exit_code(signalled(15)), 143, "SIGTERM");
    }

    #[test]
    fn success_is_distinguishable_from_failure() {
        assert_eq!(child_exit_code(exited(0)), 0);
        assert_ne!(
            child_exit_code(exited(1)),
            0,
            "a failing child must not look successful"
        );
    }
}
