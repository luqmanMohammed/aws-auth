use aws_sdk_ssooidc::config::Credentials;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use crate::utils::worker::Job;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Missing program to execute")]
    MissingProgram,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Execution failed with code: {0}")]
    ExecutionFailed(i32),
    #[error("Thread error: {0}")]
    Thread(String),
}

#[derive(Debug)]
pub struct ExecJob {
    pub account_id: String,
    pub credentials: Credentials,
    pub region: Arc<String>,
    pub arguments: Arc<[String]>,
    pub suppress_output: bool,
    pub output_base_path: Option<Arc<PathBuf>>,
}

impl ExecJob {
    pub fn validate(arguments: &[String]) -> Result<(), Error> {
        let _ = arguments.first().ok_or(Error::MissingProgram)?;
        Ok(())
    }
}

impl std::panic::UnwindSafe for ExecJob {}

impl std::panic::RefUnwindSafe for ExecJob {}

impl Job for ExecJob {
    type Error = Error;
    type Output = usize;

    fn get_job_id(&self) -> &str {
        &self.account_id
    }

    fn execute(self) -> Result<Self::Output, Self::Error> {
        if self.suppress_output {
            exec::<File, File>(
                &self.account_id,
                &self.arguments,
                self.credentials,
                &self.region,
                true,
                None,
                None,
            )
        } else if let Some(base_path) = self.output_base_path {
            let stdout_path = base_path.join(format!("{}-stdout.log", self.account_id));
            let stderr_path = base_path.join(format!("{}-stderr.log", self.account_id));
            let mut stdout_file = File::create(stdout_path)?;
            let mut stderr_file = File::create(stderr_path)?;
            exec::<File, File>(
                &self.account_id,
                &self.arguments,
                self.credentials,
                &self.region,
                false,
                Some(&mut stdout_file),
                Some(&mut stderr_file),
            )
        } else {
            exec::<File, File>(
                &self.account_id,
                &self.arguments,
                self.credentials,
                &self.region,
                false,
                None,
                None,
            )
        }
    }
}

fn exec<W1: Write + Send + 'static, W2: Write + Send + 'static>(
    account_id: &str,
    arguments: &[String],
    credentials: Credentials,
    region: &str,
    suppress_output: bool,
    redirect_stdout: Option<&mut W1>,
    redirect_stderr: Option<&mut W2>,
) -> Result<usize, Error> {
    // Create command
    let program = arguments.first().ok_or(Error::MissingProgram)?;
    let args = &arguments[1..];
    let mut command = Command::new(program);
    command.args(args);

    // Set credentials
    command.env("AWS_ACCOUNT_ID", account_id);
    command.env("AWS_REGION", region);
    command.env("AWS_ACCESS_KEY_ID", credentials.access_key_id());
    command.env("AWS_SECRET_ACCESS_KEY", credentials.secret_access_key());
    if let Some(token) = credentials.session_token() {
        command.env("AWS_SESSION_TOKEN", token);
    }

    // Configure output handling
    if suppress_output {
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
    } else {
        if redirect_stdout.is_some() {
            command.stdout(Stdio::piped());
        }
        if redirect_stderr.is_some() {
            command.stderr(Stdio::piped());
        }
    }

    // Spawn the process
    let mut child = command.spawn()?;

    thread::scope(|s| {
        let stdout_handle = if let Some(stdout_writer) = redirect_stdout {
            child.stdout.take().map(|stdout| {
                s.spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    io::copy(&mut reader, stdout_writer)
                })
            })
        } else {
            None
        };

        let stderr_handle = if let Some(stderr_writer) = redirect_stderr {
            child.stderr.take().map(|stderr| {
                s.spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    io::copy(&mut reader, stderr_writer)
                })
            })
        } else {
            None
        };

        // Wait for output processing to complete
        if let Some(handle) = stdout_handle {
            handle
                .join()
                .map_err(|_| Error::Thread("Stdout thread panicked".to_string()))?
                .map_err(Error::Io)?;
        }

        if let Some(handle) = stderr_handle {
            handle
                .join()
                .map_err(|_| Error::Thread("Stderr thread panicked".to_string()))?
                .map_err(Error::Io)?;
        }

        // Wait for the process to complete
        let status = child.wait()?;

        if !status.success() {
            return Err(Error::ExecutionFailed(status.code().unwrap_or(-1)));
        }

        Ok(status.code().unwrap_or(0) as usize)
    })
}
