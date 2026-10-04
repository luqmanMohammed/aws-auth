use std::fs::File;
use std::io::{self, PipeReader, PipeWriter, Read};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use super::finish::{self, FinishedStream};
use super::tee::{self, Route, Tee};
use super::writer::{ProcessCaptureWriter, WriteTarget};
use super::{Console, Delivery, LogPaths, Metadata, Mux, StdStream};

/// Feeds one stream from its pipe into its tee until every write end is closed.
struct Pump<M: Metadata> {
    reader: PipeReader,
    tee: Tee<M>,
}

impl<M: Metadata> Pump<M> {
    fn run(mut self) -> FinishedStream {
        let mut chunk = [0; 8 * 1024];
        let read_error = loop {
            match self.reader.read(&mut chunk) {
                Ok(0) => break None,
                Ok(read) => self.tee.feed(&chunk[..read]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => break Some(err),
            }
        };
        FinishedStream {
            read_error,
            ..self.tee.finish()
        }
    }
}

/// What a child's stdout or stderr is connected to, following [`Route`].
enum ChildStream {
    Discard,
    Direct {
        file: File,
        replayed: bool,
    },
    Pipe {
        writer: PipeWriter,
        pump: JoinHandle<FinishedStream>,
    },
}

impl ChildStream {
    fn open<M: Metadata>(
        console: Option<&Console<M>>,
        stream: StdStream,
        name: &Arc<str>,
        start: &Arc<M::Start>,
        log: Option<File>,
    ) -> io::Result<Self> {
        Ok(match tee::route(console, stream, name, start, log)? {
            Route::Discard => ChildStream::Discard,
            Route::Direct { file, replayed } => ChildStream::Direct { file, replayed },
            Route::PassThrough(tee) => {
                let (reader, writer) = io::pipe()?;
                let pump = Pump { reader, tee };
                let pump = thread::spawn(move || pump.run());
                ChildStream::Pipe { writer, pump }
            }
        })
    }

    fn stdio(&self) -> io::Result<Stdio> {
        Ok(match self {
            ChildStream::Discard => Stdio::null(),
            ChildStream::Direct { file, .. } => file.try_clone()?.into(),
            ChildStream::Pipe { writer, .. } => writer.try_clone()?.into(),
        })
    }

    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    fn writer(&self) -> io::Result<WriteTarget<'_>> {
        Ok(match self {
            ChildStream::Discard => WriteTarget::Discard,
            ChildStream::Direct { file, .. } => WriteTarget::File(file.try_clone()?),
            ChildStream::Pipe { writer, .. } => WriteTarget::Pipe(writer.try_clone()?),
        })
    }

    fn finish(self) -> io::Result<FinishedStream> {
        match self {
            ChildStream::Discard => Ok(FinishedStream::default()),
            ChildStream::Direct { file, replayed } => FinishedStream::direct(file, replayed),
            ChildStream::Pipe { writer, pump } => {
                drop(writer);
                pump.join()
                    .map_err(|_| io::Error::other("output capture thread panicked"))
            }
        }
    }
}

pub struct ProcessCapture<'a, M: Metadata> {
    mux: &'a Mux<M>,
    name: Arc<str>,
    start: Arc<M::Start>,
    out: ChildStream,
    err: ChildStream,
    logs: Option<LogPaths>,
}

impl<'a, M: Metadata> ProcessCapture<'a, M> {
    pub(super) fn open(mux: &'a Mux<M>, name: &str, start: M::Start) -> io::Result<Self> {
        let (out_log, err_log, logs) = mux.open_logs(name)?;
        let name: Arc<str> = name.into();
        let start = Arc::new(start);
        let console = mux.console.as_ref();
        let out = ChildStream::open(console, StdStream::Stdout, &name, &start, out_log)?;
        let err = ChildStream::open(console, StdStream::Stderr, &name, &start, err_log)?;
        Ok(Self {
            mux,
            name,
            start,
            out,
            err,
            logs,
        })
    }

    /// Sets the capture's stdout and stderr on `command` and lends it to `run`, which decides
    /// how it runs. The command holds a copy of both write ends until it is dropped when `run`
    /// returns, so moving it out of the borrow leaves [`ProcessCapture::finish`] waiting.
    pub fn attach<T>(
        &self,
        mut command: Command,
        run: impl FnOnce(&mut Command) -> io::Result<T>,
    ) -> io::Result<T> {
        command.stdout(self.out.stdio()?).stderr(self.err.stdio()?);
        run(&mut command)
    }

    /// For writing into the capture from this process, ordered with whatever an attached
    /// command writes to the same stream.
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    pub fn streams(&self) -> io::Result<(ProcessCaptureWriter<'_>, ProcessCaptureWriter<'_>)> {
        Ok((
            ProcessCaptureWriter(self.out.writer()?),
            ProcessCaptureWriter(self.err.writer()?),
        ))
    }

    pub fn logs(&self) -> Option<&LogPaths> {
        self.logs.as_ref()
    }

    /// Waits until everything holding a write end of a pipe has closed it, which includes
    /// processes an attached command started and left running, then writes any grouped output.
    /// Output written straight to a file is not waited for: a process left running can still be
    /// writing to it, and what it writes after this is not shown.
    pub fn finish(self, end: &M::End) -> io::Result<Delivery> {
        let out = self.out.finish()?;
        let err = self.err.finish()?;
        finish::deliver(self.mux, &self.name, &self.start, end, self.logs, out, err)
    }
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(all(test, unix))]
mod tests {
    use super::super::{GroupBuffer, Layout, LineFormatter, LogDirectory};
    use super::*;
    use crate::utils::test_support::TempDir;
    use std::fs;
    use std::io::Write;
    use std::process::ExitStatus;

    struct TestMeta;

    impl Metadata for TestMeta {
        type Start = String;
        type End = i32;
    }

    fn directory_only(dir: &TempDir) -> Mux<TestMeta> {
        Mux::new(None, Some(LogDirectory::create(dir.join("out")).unwrap()))
    }

    /// A console that prints nothing, so the output goes through the pipes without reaching
    /// the test's own stdout.
    fn piped(dir: &TempDir) -> Mux<TestMeta> {
        let silent: LineFormatter<TestMeta> = Arc::new(|_, _, _, _| None);
        Mux::new(
            Some(Console::Live { formatter: silent }),
            Some(LogDirectory::create(dir.join("out")).unwrap()),
        )
    }

    fn sh_command(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    fn sh(capture: &ProcessCapture<TestMeta>, script: &str) -> ExitStatus {
        capture
            .attach(sh_command(script), |command| command.spawn())
            .unwrap()
            .wait()
            .unwrap()
    }

    fn read(path: &std::path::Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn without_a_console_the_command_writes_straight_to_its_logs() {
        let dir = TempDir::new("capture-direct");
        let mux = directory_only(&dir);
        let capture = mux.capture_process("alpha", "start".into()).unwrap();

        assert!(sh(&capture, "echo out; echo err >&2").success());
        let logs = capture.finish(&0).unwrap().logs.unwrap();

        assert_eq!(read(&logs.stdout), "out\n");
        assert_eq!(read(&logs.stderr), "err\n");
    }

    #[test]
    fn through_the_pipes_both_streams_reach_the_logs() {
        let dir = TempDir::new("capture-piped");
        let mux = piped(&dir);
        let capture = mux.capture_process("alpha", "start".into()).unwrap();

        sh(&capture, "echo out; echo err >&2");
        let finished = capture.finish(&0).unwrap();

        assert!(finished.console_error.is_none());
        let logs = finished.logs.unwrap();
        assert_eq!(read(&logs.stdout), "out\n");
        assert_eq!(read(&logs.stderr), "err\n");
    }

    #[test]
    fn finish_waits_for_a_process_the_command_left_running() {
        let dir = TempDir::new("capture-grandchild");
        let mux = piped(&dir);
        let capture = mux.capture_process("alpha", "start".into()).unwrap();

        sh(&capture, "(sleep 0.3; echo late) &");
        let logs = capture.finish(&0).unwrap().logs.unwrap();

        assert_eq!(read(&logs.stdout), "late\n");
    }

    #[test]
    fn this_process_and_the_command_share_a_stream() {
        let dir = TempDir::new("capture-writers");
        for mux in [directory_only(&dir), piped(&dir)] {
            let capture = mux.capture_process("alpha", "start".into()).unwrap();
            {
                let (mut out, _) = capture.streams().unwrap();
                out.write_all(b"before\n").unwrap();
            }
            sh(&capture, "echo after");
            let logs = capture.finish(&0).unwrap().logs.unwrap();

            assert_eq!(read(&logs.stdout), "before\nafter\n");
        }
    }

    #[test]
    fn finish_returns_once_the_caller_has_run_the_command_its_own_way() {
        let dir = TempDir::new("capture-caller-runs");
        let mux = piped(&dir);
        let capture = mux.capture_process("alpha", "start".into()).unwrap();
        let command = sh_command("echo out");

        let status = capture.attach(command, |command| command.status()).unwrap();
        let logs = capture.finish(&0).unwrap().logs.unwrap();

        assert!(status.success());
        assert_eq!(read(&logs.stdout), "out\n");
    }

    #[test]
    fn a_log_that_cannot_be_written_is_reported_and_the_capture_still_finishes() {
        let dir = TempDir::new("capture-log-error");
        let mux = piped(&dir);
        let mut capture = mux.capture_process("alpha", "start".into()).unwrap();
        let read_only = File::open(&capture.logs.as_ref().unwrap().stdout).unwrap();
        capture.out = ChildStream::open(
            mux.console.as_ref(),
            StdStream::Stdout,
            &capture.name,
            &capture.start,
            Some(read_only),
        )
        .unwrap();

        sh(&capture, "echo out");
        let finished = capture.finish(&0).unwrap();

        assert!(finished.log_error.is_some());
        assert!(finished.console_error.is_none());
        assert!(finished.logs.is_some(), "the paths are still returned");
    }

    #[test]
    fn a_grouped_console_with_a_log_reads_the_log_back_instead_of_a_second_copy() {
        let dir = TempDir::new("capture-grouped-in-log");
        let mux = Mux::new(
            Some(Console::Grouped {
                buffer: GroupBuffer::Memory,
                layout: Layout::JsonLine,
            }),
            Some(LogDirectory::create(dir.join("out")).unwrap()),
        );
        let capture = mux.capture_process("alpha", "start".into()).unwrap();
        assert!(
            matches!(capture.out, ChildStream::Direct { replayed: true, .. }),
            "the command writes to its log with no pipe in between"
        );

        sh(&capture, "printf partial");
        let mut replay = capture.out.finish().unwrap().replay.unwrap();

        assert!(replay.ends_mid_line());
        assert_eq!(replay.read_all().unwrap(), b"partial");
    }

    #[test]
    fn several_commands_can_write_into_one_capture() {
        let dir = TempDir::new("capture-several");
        let mux = piped(&dir);
        let capture = mux.capture_process("alpha", "start".into()).unwrap();

        sh(&capture, "echo first");
        sh(&capture, "echo second");
        let logs = capture.finish(&0).unwrap().logs.unwrap();

        assert_eq!(read(&logs.stdout), "first\nsecond\n");
    }

    #[test]
    fn with_no_console_or_directory_the_command_still_runs() {
        let mux: Mux<TestMeta> = Mux::new(None, None);
        let capture = mux.capture_process("alpha", "start".into()).unwrap();

        assert!(sh(&capture, "echo discarded").success());
        assert!(capture.finish(&0).unwrap().logs.is_none());
    }
}
