use std::fs::File;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use super::finish::{self, FinishedStream};
use super::tee::{self, Route, Tee};
use super::writer::{CaptureWriter, WriteTarget};
use super::{Console, Delivery, LogPaths, Metadata, Mux, StdStream};

/// One stream of a capture fed from this process, following [`Route`]. Writers reach the tee
/// directly, so nothing in between needs a pipe or a thread.
#[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
enum ThreadStream<M: Metadata> {
    Discard,
    Direct { file: File, replayed: bool },
    PassThrough(Mutex<Tee<M>>),
}

impl<M: Metadata> ThreadStream<M> {
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    fn open(
        console: Option<&Console<M>>,
        stream: StdStream,
        name: &Arc<str>,
        start: &Arc<M::Start>,
        log: Option<File>,
    ) -> io::Result<Self> {
        Ok(match tee::route(console, stream, name, start, log)? {
            Route::Discard => ThreadStream::Discard,
            Route::Direct { file, replayed } => ThreadStream::Direct { file, replayed },
            Route::PassThrough(tee) => ThreadStream::PassThrough(Mutex::new(tee)),
        })
    }

    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    fn writer(&self) -> io::Result<CaptureWriter<'_>> {
        let target = match self {
            ThreadStream::Discard => WriteTarget::Discard,
            ThreadStream::Direct { file, .. } => WriteTarget::File(file.try_clone()?),
            ThreadStream::PassThrough(tee) => WriteTarget::Tee(tee),
        };
        Ok(CaptureWriter::new(target))
    }

    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    fn finish(self) -> io::Result<FinishedStream> {
        match self {
            ThreadStream::Discard => Ok(FinishedStream::default()),
            ThreadStream::Direct { file, replayed } => FinishedStream::direct(file, replayed),
            ThreadStream::PassThrough(tee) => Ok(tee
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner)
                .finish()),
        }
    }
}

/// The output of work done in this process, such as one or more threads, captured as a stdout
/// and a stderr of its own.
#[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
pub struct Capture<'a, M: Metadata> {
    mux: &'a Mux<M>,
    name: Arc<str>,
    start: Arc<M::Start>,
    out: ThreadStream<M>,
    err: ThreadStream<M>,
    logs: Option<LogPaths>,
}

impl<'a, M: Metadata> Capture<'a, M> {
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    pub(super) fn open(mux: &'a Mux<M>, name: &str, start: M::Start) -> io::Result<Self> {
        let (out_log, err_log, logs) = mux.open_logs(name)?;
        let name: Arc<str> = name.into();
        let start = Arc::new(start);
        let console = mux.console.as_ref();
        let out = ThreadStream::open(console, StdStream::Stdout, &name, &start, out_log)?;
        let err = ThreadStream::open(console, StdStream::Stderr, &name, &start, err_log)?;
        Ok(Self {
            mux,
            name,
            start,
            out,
            err,
            logs,
        })
    }

    /// Any number of writers, from any threads, can be open on one capture at once.
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    pub fn streams(&self) -> io::Result<(CaptureWriter<'_>, CaptureWriter<'_>)> {
        Ok((self.out.writer()?, self.err.writer()?))
    }

    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    pub fn finish(self, end: &M::End) -> io::Result<Delivery> {
        let out = self.out.finish()?;
        let err = self.err.finish()?;
        finish::deliver(self.mux, &self.name, &self.start, end, self.logs, out, err)
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::super::{GroupBuffer, Layout, LineFormatter, LogDirectory};
    use super::*;
    use crate::utils::test_support::TempDir;
    use std::fs;
    use std::io::Write;
    use std::thread;

    struct TestMeta;

    impl Metadata for TestMeta {
        type Start = String;
        type End = i32;
    }

    fn logged(dir: &TempDir, console: Option<Console<TestMeta>>) -> Mux<TestMeta> {
        Mux::new(
            console,
            Some(LogDirectory::create(dir.join("out")).unwrap()),
        )
    }

    /// A live console that prints nothing, so lines go through the tee without reaching the
    /// test's own stdout.
    fn silent() -> Console<TestMeta> {
        let silent: LineFormatter<TestMeta> = Arc::new(|_, _, _, _| None);
        Console::Live { formatter: silent }
    }

    #[test]
    fn without_a_console_writes_go_straight_to_the_logs() {
        let dir = TempDir::new("stdmux-capture-direct");
        let mux = logged(&dir, None);
        let capture = mux.capture("alpha", "start".into()).unwrap();
        assert!(matches!(capture.out, ThreadStream::Direct { .. }));

        let (mut out, mut err) = capture.streams().unwrap();
        writeln!(out, "out").unwrap();
        writeln!(err, "err").unwrap();
        drop((out, err));
        let logs = capture.finish(&0).unwrap().logs.unwrap();

        assert_eq!(fs::read_to_string(logs.stdout).unwrap(), "out\n");
        assert_eq!(fs::read_to_string(logs.stderr).unwrap(), "err\n");
    }

    #[test]
    fn lines_from_many_threads_sharing_a_stream_stay_whole() {
        let dir = TempDir::new("stdmux-capture-threads");
        let mux = logged(&dir, Some(silent()));
        let capture = mux.capture("alpha", "start".into()).unwrap();

        thread::scope(|s| {
            for worker in 0..4 {
                let capture = &capture;
                s.spawn(move || {
                    let (mut out, _) = capture.streams().unwrap();
                    for line in 0..200 {
                        writeln!(out, "worker {worker} line {line}").unwrap();
                    }
                });
            }
        });
        let logs = capture.finish(&0).unwrap().logs.unwrap();

        let written = fs::read_to_string(logs.stdout).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 4 * 200);
        assert!(
            lines
                .iter()
                .all(|line| line.starts_with("worker ") && line.split(' ').count() == 4),
            "a line was split or mixed with another"
        );
    }

    #[test]
    fn an_unfinished_line_is_written_when_its_writer_is_dropped() {
        let dir = TempDir::new("stdmux-capture-partial");
        let mux = logged(&dir, Some(silent()));
        let capture = mux.capture("alpha", "start".into()).unwrap();

        let (mut out, _) = capture.streams().unwrap();
        write!(out, "no newline").unwrap();
        drop(out);
        let logs = capture.finish(&0).unwrap().logs.unwrap();

        assert_eq!(fs::read_to_string(logs.stdout).unwrap(), "no newline");
    }

    #[test]
    fn a_grouped_console_in_memory_keeps_what_was_written_for_the_end() {
        let mux: Mux<TestMeta> = Mux::new(
            Some(Console::Grouped {
                buffer: GroupBuffer::Memory,
                layout: Layout::JsonLine,
            }),
            None,
        );
        let capture = mux.capture("alpha", "start".into()).unwrap();
        assert!(matches!(capture.out, ThreadStream::PassThrough(_)));

        let (mut out, _) = capture.streams().unwrap();
        writeln!(out, "held").unwrap();
        drop(out);
        let mut replay = capture.out.finish().unwrap().replay.unwrap();

        assert_eq!(replay.read_all().unwrap(), b"held\n");
    }
}
