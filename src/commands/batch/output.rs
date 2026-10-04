use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::cmd::OutputMode;
use crate::utils::private_fs;
use crate::utils::stdmux::{Console, GroupBuffer, Layout, LogDirectory, LogPaths, Metadata, Mux};

const RESULTS_FILE: &str = "results.jsonl";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Failed,
    Skipped,
    Unresolved,
}

#[derive(Serialize)]
pub struct AccountStart {
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct AccountEnd {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AccountEnd {
    pub fn not_run(status: Status) -> Self {
        Self {
            status,
            exit_code: None,
            error: None,
        }
    }

    pub fn failed(error: impl ToString) -> Self {
        Self {
            error: Some(error.to_string()),
            ..Self::not_run(Status::Failed)
        }
    }

    /// Keeps the exit code, for a command that ran but whose account still failed afterwards.
    pub fn failed_after(self, error: impl ToString) -> Self {
        Self {
            status: Status::Failed,
            error: Some(error.to_string()),
            ..self
        }
    }

    pub fn from_run(run: &io::Result<ExitStatus>) -> Self {
        match run {
            Ok(status) => Self {
                status: if status.success() {
                    Status::Ok
                } else {
                    Status::Failed
                },
                exit_code: status.code(),
                error: status.code().is_none().then(|| status.to_string()),
            },
            Err(err) => Self::failed(err),
        }
    }

    fn describe(&self) -> String {
        match (self.status, self.exit_code, &self.error) {
            (Status::Ok, ..) => "ok".to_string(),
            (_, Some(code), _) => format!("failed, exit {code}"),
            (_, None, Some(error)) => format!("failed: {error}"),
            (status, None, None) => format!("{status:?}").to_lowercase(),
        }
    }
}

pub struct AccountMeta;

impl Metadata for AccountMeta {
    type Start = AccountStart;
    type End = AccountEnd;
}

fn console(mode: &OutputMode) -> Option<Console<AccountMeta>> {
    match mode {
        OutputMode::Group => Some(Console::Grouped {
            buffer: GroupBuffer::TempFile,
            layout: Layout::Block {
                header: Arc::new(|account, start, end| {
                    format!("==> {account} ({}) {}\n", start.role, end.describe()).into_bytes()
                }),
            },
        }),
        OutputMode::Tag => Some(Console::Live {
            formatter: Arc::new(|account, _, _, line| {
                Some([b"[", account.as_bytes(), b"] ", line].concat())
            }),
        }),
        OutputMode::Json => Some(Console::Grouped {
            buffer: GroupBuffer::TempFile,
            layout: Layout::JsonLine,
        }),
        OutputMode::Raw | OutputMode::None => None,
    }
}

/// How each account's command is connected to this process's stdio.
pub enum ChildStdio {
    /// The command keeps the terminal, stdin included, and nothing passes through this process.
    Inherited,
    Captured(Mux<AccountMeta>),
}

#[derive(Serialize)]
struct ResultLine<'a> {
    account_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'a str>,
    #[serde(flatten)]
    end: &'a AccountEnd,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr: Option<String>,
}

fn file_name(path: &std::path::Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Everything a run's accounts share: how their output is shown, and the results file listing
/// each of them when there is an output directory.
pub struct BatchOutput {
    pub stdio: ChildStdio,
    results: Option<Mutex<File>>,
}

impl BatchOutput {
    /// Creates the output directory up front, so a bad path fails before anything runs.
    pub fn new(mode: &OutputMode, dir: Option<PathBuf>) -> io::Result<Self> {
        if matches!(mode, OutputMode::Raw) {
            return Ok(Self {
                stdio: ChildStdio::Inherited,
                results: None,
            });
        }
        let (directory, results) = match dir {
            Some(dir) => {
                let results = dir.join(RESULTS_FILE);
                let directory = LogDirectory::create(dir)?;
                let results = private_fs::create_replacing(&results)?;
                (Some(directory), Some(Mutex::new(results)))
            }
            None => (None, None),
        };
        Ok(Self {
            stdio: ChildStdio::Captured(Mux::new(console(mode), directory)),
            results,
        })
    }

    /// Adds one account to the results file, naming its logs relative to the directory.
    pub fn record(
        &self,
        account_id: &str,
        role: Option<&str>,
        end: &AccountEnd,
        logs: Option<&LogPaths>,
    ) -> io::Result<()> {
        let Some(results) = &self.results else {
            return Ok(());
        };
        let line = ResultLine {
            account_id,
            role,
            end,
            stdout: logs.and_then(|logs| file_name(&logs.stdout)),
            stderr: logs.and_then(|logs| file_name(&logs.stderr)),
        };
        let mut line = serde_json::to_vec(&line)?;
        line.push(b'\n');
        results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_all(&line)
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;
    use std::fs;

    fn results_of(dir: &TempDir) -> Vec<serde_json::Value> {
        fs::read_to_string(dir.join(RESULTS_FILE))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn raw_keeps_the_terminal_and_records_nothing() {
        let output = BatchOutput::new(&OutputMode::Raw, None).unwrap();

        assert!(matches!(output.stdio, ChildStdio::Inherited));
        assert!(output.results.is_none());
    }

    #[test]
    fn a_missing_output_directory_is_created_with_an_empty_results_file() {
        let dir = TempDir::new("batch-output-mkdir");
        let out = dir.join("a/b");

        BatchOutput::new(&OutputMode::None, Some(out.clone())).unwrap();

        assert_eq!(fs::read_to_string(out.join(RESULTS_FILE)).unwrap(), "");
    }

    #[test]
    fn a_result_line_names_the_logs_relative_to_the_directory() {
        let dir = TempDir::new("batch-output-results");
        let output = BatchOutput::new(&OutputMode::None, Some(dir.path().to_path_buf())).unwrap();
        let logs = LogPaths {
            stdout: dir.join("111111111111-stdout.log"),
            stderr: dir.join("111111111111-stderr.log"),
        };
        let failed = AccountEnd {
            exit_code: Some(3),
            ..AccountEnd::not_run(Status::Failed)
        };

        output
            .record("111111111111", Some("AdminRole"), &failed, Some(&logs))
            .unwrap();
        output
            .record(
                "222222222222",
                None,
                &AccountEnd::not_run(Status::Unresolved),
                None,
            )
            .unwrap();

        assert_eq!(
            results_of(&dir),
            vec![
                serde_json::json!({
                    "account_id": "111111111111",
                    "role": "AdminRole",
                    "status": "failed",
                    "exit_code": 3,
                    "stdout": "111111111111-stdout.log",
                    "stderr": "111111111111-stderr.log",
                }),
                serde_json::json!({"account_id": "222222222222", "status": "unresolved"}),
            ]
        );
    }

    #[test]
    fn an_account_end_says_how_the_account_ended() {
        let failed = AccountEnd {
            exit_code: Some(3),
            ..AccountEnd::not_run(Status::Failed)
        };
        assert_eq!(failed.describe(), "failed, exit 3");
        assert_eq!(
            AccountEnd::failed("no such program").describe(),
            "failed: no such program"
        );
        assert_eq!(AccountEnd::not_run(Status::Ok).describe(), "ok");
    }

    #[test]
    fn an_account_failing_after_its_command_ran_keeps_the_exit_code() {
        let ran = AccountEnd {
            exit_code: Some(0),
            ..AccountEnd::not_run(Status::Ok)
        };

        let end = ran.failed_after("disk full");

        assert_eq!(end.status, Status::Failed);
        assert_eq!(end.exit_code, Some(0));
        assert_eq!(end.error.as_deref(), Some("disk full"));
    }
}
