mod capture;
mod console;
mod finish;
mod log_directory;
mod process_capture;
mod replay;
mod tee;
mod writer;

use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

pub use capture::Capture;
pub use console::{Console, Layout};
pub use log_directory::LogDirectory;
pub use process_capture::ProcessCapture;
pub use replay::GroupBuffer;
#[allow(unused_imports, reason = "stdmux API that aws-auth does not use yet")]
pub use writer::{CaptureWriter, ProcessCaptureWriter};

/// What a capture is described by: `Start` when it begins, `End` once it has finished.
pub trait Metadata: 'static {
    type Start: Serialize + Send + Sync + 'static;
    type End: Serialize + Send + Sync + 'static;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdStream {
    Stdout,
    Stderr,
}

/// Given a line without its newline, returns what to write for it, which is always written
/// ending in one so lines stay whole on the console, or `None` to drop the line.
pub type LineFormatter<M> =
    Arc<dyn Fn(&str, &<M as Metadata>::Start, StdStream, &[u8]) -> Option<Vec<u8>> + Send + Sync>;

pub type HeaderFormatter<M> =
    Arc<dyn Fn(&str, &<M as Metadata>::Start, &<M as Metadata>::End) -> Vec<u8> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct LogPaths {
    pub stdout: PathBuf,
    pub stderr: PathBuf,
}

/// Log and console errors are reported rather than returned, so one failing still leaves the
/// other written; what a failure means is the caller's call. Only output that could not be read
/// fails [`ProcessCapture::finish`], since then neither is complete.
#[derive(Debug)]
pub struct Delivery {
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    pub logs: Option<LogPaths>,
    /// Only for logs written through a pipe; a command given its log directly sees its own
    /// write errors.
    pub log_error: Option<io::Error>,
    pub console_error: Option<io::Error>,
}

pub struct Mux<M: Metadata> {
    console: Option<Console<M>>,
    directory: Option<LogDirectory>,
}

impl<M: Metadata> Mux<M> {
    pub fn new(console: Option<Console<M>>, directory: Option<LogDirectory>) -> Self {
        Self { console, directory }
    }

    /// For output written from this process, by one thread or many.
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    pub fn capture(&self, name: &str, start: M::Start) -> io::Result<Capture<'_, M>> {
        Capture::open(self, name, start)
    }

    /// For the output of child processes, which reaches this process through a pipe whenever
    /// something needs to see it.
    pub fn capture_process(
        &self,
        name: &str,
        start: M::Start,
    ) -> io::Result<ProcessCapture<'_, M>> {
        ProcessCapture::open(self, name, start)
    }

    fn open_logs(&self, name: &str) -> io::Result<(Option<File>, Option<File>, Option<LogPaths>)> {
        Ok(match &self.directory {
            Some(directory) => {
                let (out, err, paths) = directory.open_logs(name)?;
                (Some(out), Some(err), Some(paths))
            }
            None => (None, None, None),
        })
    }
}
