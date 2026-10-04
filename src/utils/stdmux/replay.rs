use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

/// Where a console that shows each capture's output only once it finishes keeps that output, when
/// there is no log that already holds it.
#[derive(Clone, Copy, Debug)]
pub enum GroupBuffer {
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    Memory,
    /// An owner-only file that never has a name on disk, or loses it as soon as it is open,
    /// so buffered output is not left behind even if the process is killed.
    TempFile,
}

enum ReplaySource {
    Memory(Vec<u8>),
    File(File),
}

pub(super) struct Replay {
    source: ReplaySource,
    last: Option<u8>,
}

impl Replay {
    pub(super) fn in_memory() -> Self {
        Self {
            source: ReplaySource::Memory(Vec::new()),
            last: None,
        }
    }

    /// For a file something else has already written, such as a log a command wrote to directly.
    pub(super) fn from_file(mut file: File) -> io::Result<Self> {
        let last = if file.metadata()?.len() == 0 {
            None
        } else {
            let mut last = [0];
            file.seek(SeekFrom::End(-1))?;
            file.read_exact(&mut last)?;
            Some(last[0])
        };
        Ok(Self {
            source: ReplaySource::File(file),
            last,
        })
    }

    pub(super) fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match &mut self.source {
            ReplaySource::Memory(source) => source.extend_from_slice(bytes),
            ReplaySource::File(file) => file.write_all(bytes)?,
        }
        if let Some(&last) = bytes.last() {
            self.last = Some(last);
        }
        Ok(())
    }

    pub(super) fn ends_mid_line(&self) -> bool {
        self.last.is_some_and(|last| last != b'\n')
    }

    pub(super) fn copy_to(&mut self, writer: &mut dyn Write) -> io::Result<()> {
        match &mut self.source {
            ReplaySource::Memory(source) => writer.write_all(source),
            ReplaySource::File(file) => {
                file.seek(SeekFrom::Start(0))?;
                io::copy(file, writer).map(|_| ())
            }
        }
    }

    pub(super) fn read_all(&mut self) -> io::Result<Vec<u8>> {
        match &mut self.source {
            ReplaySource::Memory(source) => Ok(std::mem::take(source)),
            ReplaySource::File(file) => {
                file.seek(SeekFrom::Start(0))?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Ok(bytes)
            }
        }
    }
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> Replay {
        let mut replay = Replay::in_memory();
        replay.write_all(b"hello ").unwrap();
        replay.write_all(b"world\n").unwrap();
        replay
    }

    fn in_file() -> Replay {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"hello world\n").unwrap();
        Replay::from_file(file).unwrap()
    }

    #[test]
    fn memory_and_file_give_back_what_was_written() {
        for (kind, replay) in [("memory", in_memory as fn() -> Replay), ("file", in_file)] {
            let mut copied = Vec::new();
            replay().copy_to(&mut copied).unwrap();
            assert_eq!(copied, b"hello world\n", "{kind} copy");

            assert_eq!(
                replay().read_all().unwrap(),
                b"hello world\n",
                "{kind} read"
            );
        }
    }

    #[test]
    fn only_output_that_stops_mid_line_ends_mid_line() {
        let mut store = Replay::in_memory();
        assert!(!store.ends_mid_line(), "nothing written");

        store.write_all(b"partial").unwrap();
        assert!(store.ends_mid_line());

        store.write_all(b" line\n").unwrap();
        store.write_all(b"").unwrap();
        assert!(!store.ends_mid_line(), "an empty write changes nothing");
    }

    #[test]
    fn a_store_read_back_from_a_written_file_knows_how_it_ends() {
        for (written, mid_line) in [(&b"partial"[..], true), (b"whole\n", false), (b"", false)] {
            let mut file = tempfile::tempfile().unwrap();
            file.write_all(written).unwrap();

            let mut store = Replay::from_file(file).unwrap();

            assert_eq!(store.ends_mid_line(), mid_line, "{written:?}");
            assert_eq!(store.read_all().unwrap(), written, "{written:?}");
        }
    }
}
