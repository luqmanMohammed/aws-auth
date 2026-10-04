use std::fs::File;
use std::io::{self, Write};
use std::sync::Arc;

use crate::utils::private_fs;

use super::console::{self, LineSplitter};
use super::finish::FinishedStream;
use super::replay::Replay;
use super::{Console, GroupBuffer, LineFormatter, Metadata, StdStream};

/// Where one stream's bytes are stored.
enum Storage {
    Nothing,
    Log(File),
    TempFile(File),
    Memory(Replay),
}

impl Storage {
    /// A log wins, since it already holds everything a grouped console would buffer.
    fn new<M: Metadata>(console: Option<&Console<M>>, log: Option<File>) -> io::Result<Self> {
        Ok(match (log, console) {
            (Some(log), _) => Storage::Log(log),
            (None, Some(Console::Grouped { buffer, .. })) => match buffer {
                GroupBuffer::TempFile => Storage::TempFile(private_fs::temp_file()?),
                GroupBuffer::Memory => Storage::Memory(Replay::in_memory()),
            },
            (None, _) => Storage::Nothing,
        })
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Storage::Nothing => Ok(()),
            Storage::Log(file) | Storage::TempFile(file) => file.write_all(bytes),
            Storage::Memory(replay) => replay.write_all(bytes),
        }
    }

    fn into_replay(self) -> io::Result<Option<Replay>> {
        Ok(match self {
            Storage::Nothing => None,
            Storage::Log(file) | Storage::TempFile(file) => Some(Replay::from_file(file)?),
            Storage::Memory(replay) => Some(replay),
        })
    }
}

struct LivePrinter<M: Metadata> {
    formatter: LineFormatter<M>,
    name: Arc<str>,
    start: Arc<M::Start>,
    splitter: LineSplitter,
}

impl<M: Metadata> LivePrinter<M> {
    fn new(console: Option<&Console<M>>, name: &Arc<str>, start: &Arc<M::Start>) -> Option<Self> {
        match console {
            Some(Console::Live { formatter }) => Some(Self {
                formatter: formatter.clone(),
                name: name.clone(),
                start: start.clone(),
                splitter: LineSplitter::default(),
            }),
            _ => None,
        }
    }

    fn push(&mut self, stream: StdStream, bytes: &[u8]) -> io::Result<()> {
        let Self {
            formatter,
            name,
            start,
            splitter,
        } = self;
        splitter.push(bytes, |line| {
            console::print_line::<M>(formatter, name, start, stream, line)
        })
    }

    fn end(&mut self, stream: StdStream) -> io::Result<()> {
        let Self {
            formatter,
            name,
            start,
            splitter,
        } = self;
        splitter.finish(|line| console::print_line::<M>(formatter, name, start, stream, line))
    }
}

/// Everything one stream's bytes go to: where they are stored, and a live console. A failed
/// write is recorded rather than returned, so whatever feeds the tee keeps going, and a pipe
/// behind it is always drained.
pub(super) struct Tee<M: Metadata> {
    stream: StdStream,
    storage: Storage,
    printer: Option<LivePrinter<M>>,
    replayed: bool,
    log_error: Option<io::Error>,
    console_error: Option<io::Error>,
}

impl<M: Metadata> Tee<M> {
    pub(super) fn feed(&mut self, bytes: &[u8]) {
        if let Err(err) = self.storage.write(bytes) {
            let slot = match self.storage {
                Storage::Log(_) => &mut self.log_error,
                _ => &mut self.console_error,
            };
            slot.get_or_insert(err);
        }
        if self.console_error.is_none()
            && let Some(printer) = &mut self.printer
            && let Err(err) = printer.push(self.stream, bytes)
        {
            self.console_error = Some(err);
        }
    }

    pub(super) fn finish(mut self) -> FinishedStream {
        if self.console_error.is_none()
            && let Some(printer) = &mut self.printer
            && let Err(err) = printer.end(self.stream)
        {
            self.console_error = Some(err);
        }
        let replay = if self.replayed {
            match self.storage.into_replay() {
                Ok(replay) => replay,
                Err(err) => {
                    self.console_error.get_or_insert(err);
                    None
                }
            }
        } else {
            None
        };
        FinishedStream {
            replay,
            read_error: None,
            log_error: self.log_error,
            console_error: self.console_error,
        }
    }
}

impl<M: Metadata> Write for Tee<M> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.feed(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// How one stream is connected. The bytes only pass through this process when something reads
/// them as they arrive, or when they are stored in memory; otherwise whatever produces them
/// writes straight to where they are stored.
pub(super) enum Route<M: Metadata> {
    Discard,
    /// `replayed` is set for a grouped console, which reads the file back once the capture ends.
    Direct {
        file: File,
        replayed: bool,
    },
    PassThrough(Tee<M>),
}

pub(super) fn route<M: Metadata>(
    console: Option<&Console<M>>,
    stream: StdStream,
    name: &Arc<str>,
    start: &Arc<M::Start>,
    log: Option<File>,
) -> io::Result<Route<M>> {
    let replayed = matches!(console, Some(Console::Grouped { .. }));
    let storage = Storage::new(console, log)?;
    let printer = LivePrinter::new(console, name, start);
    Ok(match (storage, printer) {
        (Storage::Nothing, None) => Route::Discard,
        (Storage::Log(file) | Storage::TempFile(file), None) => Route::Direct { file, replayed },
        (storage, printer) => Route::PassThrough(Tee {
            stream,
            storage,
            printer,
            replayed,
            log_error: None,
            console_error: None,
        }),
    })
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::super::Layout;
    use super::*;
    use crate::utils::test_support::TempDir;

    struct TestMeta;

    impl Metadata for TestMeta {
        type Start = String;
        type End = i32;
    }

    fn grouped(buffer: GroupBuffer) -> Console<TestMeta> {
        Console::Grouped {
            buffer,
            layout: Layout::JsonLine,
        }
    }

    fn live() -> Console<TestMeta> {
        Console::Live {
            formatter: Arc::new(|_, _, _, _| None),
        }
    }

    fn route_for(console: Option<&Console<TestMeta>>, log: Option<File>) -> Route<TestMeta> {
        route(
            console,
            StdStream::Stdout,
            &Arc::from("alpha"),
            &Arc::new("start".to_string()),
            log,
        )
        .unwrap()
    }

    fn tee(route: Route<TestMeta>) -> Tee<TestMeta> {
        let Route::PassThrough(tee) = route else {
            panic!("expected a pass-through")
        };
        tee
    }

    #[test]
    fn bytes_only_pass_through_this_process_when_they_have_to() {
        let log = || Some(tempfile::tempfile().unwrap());
        let cases = [
            ("nothing wants them", None, None, "discard"),
            ("only a log", None, log(), "direct"),
            (
                "grouped, with a log",
                Some(grouped(GroupBuffer::Memory)),
                log(),
                "direct+replayed",
            ),
            (
                "grouped, in a temp file",
                Some(grouped(GroupBuffer::TempFile)),
                None,
                "direct+replayed",
            ),
            (
                "grouped, in memory",
                Some(grouped(GroupBuffer::Memory)),
                None,
                "pass-through",
            ),
            ("live", Some(live()), None, "pass-through"),
            ("live, with a log", Some(live()), log(), "pass-through"),
        ];
        for (case, console, log, expected) in cases {
            let got = match route_for(console.as_ref(), log) {
                Route::Discard => "discard",
                Route::Direct {
                    replayed: false, ..
                } => "direct",
                Route::Direct { replayed: true, .. } => "direct+replayed",
                Route::PassThrough(_) => "pass-through",
            };
            assert_eq!(got, expected, "{case}");
        }
    }

    #[test]
    fn a_grouped_console_in_memory_gets_back_everything_written() {
        let mut tee = tee(route_for(Some(&grouped(GroupBuffer::Memory)), None));

        tee.feed(b"hello ");
        tee.feed(b"world\n");
        let result = tee.finish();

        assert!(result.log_error.is_none() && result.console_error.is_none());
        assert_eq!(result.replay.unwrap().read_all().unwrap(), b"hello world\n");
    }

    #[test]
    fn a_log_that_cannot_be_written_does_not_stop_the_live_console() {
        let dir = TempDir::new("stdmux-tee-log-error");
        let path = dir.join("out.log");
        File::create(&path).unwrap();
        let mut tee = tee(route_for(Some(&live()), Some(File::open(&path).unwrap())));

        tee.feed(b"first\n");
        tee.feed(b"second\n");
        let result = tee.finish();

        assert!(result.log_error.is_some());
        assert!(result.console_error.is_none());
        assert!(result.replay.is_none(), "a live console replays nothing");
    }

    #[test]
    #[cfg(unix)]
    fn a_temp_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let Route::Direct { file, .. } = route_for(Some(&grouped(GroupBuffer::TempFile)), None)
        else {
            panic!("expected a direct route")
        };

        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
}
