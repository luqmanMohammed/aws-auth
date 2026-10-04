use aws_sdk_ssooidc::config::Credentials;
use std::io::{self, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;

use super::output::{AccountEnd, AccountMeta, AccountStart, BatchOutput, ChildStdio};
use crate::utils::stdmux::{LogPaths, Mux};
use crate::utils::worker::Job;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Missing program to execute")]
    MissingProgram,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Execution failed with code: {0}")]
    ExecutionFailed(i32),
    #[error("Its log could not be fully written: {0}")]
    Log(io::Error),
    #[error("Its result could not be recorded: {0}")]
    Results(io::Error),
}

fn outcome(run: io::Result<ExitStatus>) -> Result<(), Error> {
    let status = run?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::ExecutionFailed(status.code().unwrap_or(-1)))
    }
}

pub struct ExecJob {
    pub account_id: String,
    pub role: String,
    pub credentials: Credentials,
    pub region: Arc<String>,
    pub arguments: Arc<[String]>,
    pub output: Arc<BatchOutput>,
}

impl std::fmt::Debug for ExecJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecJob")
            .field("account_id", &self.account_id)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

impl std::panic::UnwindSafe for ExecJob {}

impl std::panic::RefUnwindSafe for ExecJob {}

impl ExecJob {
    pub fn validate(arguments: &[String]) -> Result<(), Error> {
        let _ = arguments.first().ok_or(Error::MissingProgram)?;
        Ok(())
    }

    fn command(&self) -> Result<Command, Error> {
        let program = self.arguments.first().ok_or(Error::MissingProgram)?;
        let mut command = Command::new(program);
        command.args(&self.arguments[1..]);

        command.env("AWS_ACCOUNT_ID", &self.account_id);
        command.env("AWS_REGION", self.region.as_str());
        command.env("AWS_ACCESS_KEY_ID", self.credentials.access_key_id());
        command.env(
            "AWS_SECRET_ACCESS_KEY",
            self.credentials.secret_access_key(),
        );
        if let Some(token) = self.credentials.session_token() {
            command.env("AWS_SESSION_TOKEN", token);
        }
        Ok(command)
    }

    /// Returns how the account ended and where its logs are, alongside whether it failed.
    fn run_captured(
        &self,
        mux: &Mux<AccountMeta>,
        mut command: Command,
    ) -> (AccountEnd, Option<LogPaths>, Result<(), Error>) {
        command.stdin(Stdio::null());
        let start = AccountStart {
            role: self.role.clone(),
        };
        let capture = match mux.capture_process(&self.account_id, start) {
            Ok(capture) => capture,
            Err(err) => return (AccountEnd::failed(&err), None, Err(Error::Io(err))),
        };
        let logs = capture.logs().cloned();
        let status = capture.attach(command, |command| command.status());
        let end = AccountEnd::from_run(&status);
        let failure = match capture.finish(&end) {
            Ok(delivery) => {
                if let Some(err) = &delivery.console_error {
                    let _ = writeln!(
                        io::stderr(),
                        "WARN: account {}: its output could not be shown: {err}",
                        self.account_id
                    );
                }
                delivery.log_error.map(Error::Log)
            }
            Err(err) => Some(Error::Io(err)),
        };
        match failure {
            Some(err) => (end.failed_after(&err), logs, outcome(status).and(Err(err))),
            None => (end, logs, outcome(status)),
        }
    }
}

impl Job for ExecJob {
    type Error = Error;
    type Output = ();

    fn get_job_id(&self) -> &str {
        &self.account_id
    }

    fn execute(self) -> Result<Self::Output, Self::Error> {
        let command = match self.command() {
            Ok(command) => command,
            Err(err) => {
                let recorded = self.output.record(
                    &self.account_id,
                    Some(&self.role),
                    &AccountEnd::failed(&err),
                    None,
                );
                return Err::<(), _>(err).and(recorded.map_err(Error::Results));
            }
        };
        let (end, logs, result) = match &self.output.stdio {
            ChildStdio::Inherited => {
                let mut command = command;
                let status = command.status();
                (AccountEnd::from_run(&status), None, outcome(status))
            }
            ChildStdio::Captured(mux) => self.run_captured(mux, command),
        };
        let recorded = self
            .output
            .record(&self.account_id, Some(&self.role), &end, logs.as_ref());
        result.and(recorded.map_err(Error::Results))
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::cmd::OutputMode;
    use crate::utils::test_support::TempDir;
    use std::fs;

    fn job(output: BatchOutput, script: &str) -> ExecJob {
        ExecJob {
            account_id: "111111111111".to_string(),
            role: "AdminRole".to_string(),
            credentials: Credentials::new("AKID", "SECRET", None, None, "test"),
            region: Arc::new("eu-west-2".to_string()),
            arguments: Arc::from(vec!["sh".into(), "-c".into(), script.into()]),
            output: Arc::new(output),
        }
    }

    fn result_of(dir: &TempDir) -> serde_json::Value {
        serde_json::from_str(
            fs::read_to_string(dir.join("results.jsonl"))
                .unwrap()
                .trim(),
        )
        .unwrap()
    }

    #[test]
    fn a_failing_account_leaves_its_logs_and_a_result_line() {
        let dir = TempDir::new("batch-exec-failing");
        let output = BatchOutput::new(&OutputMode::None, Some(dir.path().to_path_buf())).unwrap();

        let result = job(output, "echo out; echo err >&2; exit 3").execute();

        assert!(
            matches!(result, Err(Error::ExecutionFailed(3))),
            "{result:?}"
        );
        assert_eq!(
            fs::read_to_string(dir.join("111111111111-stdout.log")).unwrap(),
            "out\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("111111111111-stderr.log")).unwrap(),
            "err\n"
        );
        let line = result_of(&dir);
        assert_eq!(line["status"], "failed");
        assert_eq!(line["exit_code"], 3);
        assert_eq!(line["role"], "AdminRole");
        assert_eq!(line["stdout"], "111111111111-stdout.log");
    }

    #[test]
    fn a_command_that_cannot_start_is_recorded_as_failed() {
        let dir = TempDir::new("batch-exec-spawn");
        let output = BatchOutput::new(&OutputMode::None, Some(dir.path().to_path_buf())).unwrap();
        let mut job = job(output, "");
        job.arguments = Arc::from(vec!["aws-auth-no-such-program".to_string()]);

        let result = job.execute();

        assert!(matches!(result, Err(Error::Io(_))), "{result:?}");
        let line = result_of(&dir);
        assert_eq!(line["status"], "failed");
        assert!(line["error"].is_string(), "{line}");
        assert!(line.get("exit_code").is_none(), "{line}");
    }

    #[test]
    fn a_captured_command_does_not_read_the_callers_stdin() {
        let dir = TempDir::new("batch-exec-stdin");
        let output = BatchOutput::new(&OutputMode::None, Some(dir.path().to_path_buf())).unwrap();

        job(output, "cat")
            .execute()
            .expect("cat of /dev/null ends at once");

        assert_eq!(
            fs::read_to_string(dir.join("111111111111-stdout.log")).unwrap(),
            ""
        );
    }
}
