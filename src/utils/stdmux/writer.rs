use std::fs::File;
use std::io::{self, PipeWriter, Write};
use std::sync::{Mutex, PoisonError};

use super::console::MAX_LINE;

#[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
pub(super) enum WriteTarget<'a> {
    Discard,
    File(File),
    Pipe(PipeWriter),
    Tee(&'a Mutex<dyn Write + Send + 'a>),
}

impl Write for WriteTarget<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            WriteTarget::Discard => Ok(buf.len()),
            WriteTarget::File(file) => file.write(buf),
            WriteTarget::Pipe(pipe) => pipe.write(buf),
            WriteTarget::Tee(tee) => tee
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            WriteTarget::Discard => Ok(()),
            WriteTarget::File(file) => file.flush(),
            WriteTarget::Pipe(pipe) => pipe.flush(),
            WriteTarget::Tee(tee) => tee.lock().unwrap_or_else(PoisonError::into_inner).flush(),
        }
    }
}

/// Writes into a [`Capture`](super::Capture) a line at a time: every complete line, and
/// whatever of it was written before, goes out in one write, so lines from writers sharing a
/// stream stay whole. Only a line longer than [`MAX_LINE`] is split. An unfinished line is
/// written when the writer is flushed or dropped. Borrows its capture, so it cannot still be
/// open when the capture finishes.
#[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
pub struct CaptureWriter<'a> {
    target: WriteTarget<'a>,
    pending: Vec<u8>,
}

impl<'a> CaptureWriter<'a> {
    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    pub(super) fn new(target: WriteTarget<'a>) -> Self {
        Self {
            target,
            pending: Vec::new(),
        }
    }

    #[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
    fn write_pending(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            self.target.write_all(&self.pending)?;
            self.pending.clear();
        }
        Ok(())
    }
}

impl Write for CaptureWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let before = self.pending.len();
        let complete = match buf.iter().rposition(|&byte| byte == b'\n') {
            Some(last) => last + 1,
            None if before + buf.len() >= MAX_LINE => buf.len(),
            None => {
                self.pending.extend_from_slice(buf);
                return Ok(buf.len());
            }
        };
        self.pending.extend_from_slice(&buf[..complete]);
        if let Err(err) = self.target.write_all(&self.pending) {
            self.pending.truncate(before);
            return Err(err);
        }
        self.pending.clear();
        self.pending.extend_from_slice(&buf[complete..]);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write_pending()?;
        self.target.flush()
    }
}

impl Drop for CaptureWriter<'_> {
    fn drop(&mut self) {
        let _ = self.write_pending();
    }
}

/// Writes into a [`ProcessCapture`](super::ProcessCapture) unbuffered, so it stays in order with
/// what an attached command writes to the same stream. Borrows its capture, so it cannot still
/// be open when the capture finishes.
#[allow(dead_code, reason = "stdmux API that aws-auth does not use yet")]
pub struct ProcessCaptureWriter<'a>(pub(super) WriteTarget<'a>);

impl Write for ProcessCaptureWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps each write it receives separately, so a test can see how writes were split.
    #[derive(Default)]
    struct Writes(Vec<Vec<u8>>);

    impl Write for Writes {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.push(buf.to_vec());
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn writes_of(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let writes = Mutex::new(Writes::default());
        {
            let mut writer = CaptureWriter::new(WriteTarget::Tee(&writes));
            for chunk in chunks {
                writer.write_all(chunk).unwrap();
            }
        }
        writes.into_inner().unwrap().0
    }

    #[test]
    fn each_complete_line_goes_out_in_one_write() {
        assert_eq!(
            writes_of(&[b"hel", b"lo\nwor", b"ld\n"]),
            [b"hello\n".to_vec(), b"world\n".to_vec()]
        );
    }

    #[test]
    fn an_unfinished_line_goes_out_when_the_writer_is_dropped() {
        assert_eq!(
            writes_of(&[b"done\npart", b"ial"]),
            [b"done\n".to_vec(), b"partial".to_vec()]
        );
    }

    #[test]
    fn a_line_reaching_the_limit_goes_out_without_waiting_for_its_end() {
        let long = vec![b'x'; MAX_LINE];

        let writes = writes_of(&[&long[..10], &long[10..]]);

        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].len(), MAX_LINE);
    }
}
