use std::borrow::Cow;
use std::io::{self, Write};

use serde::Serialize;

use super::replay::{GroupBuffer, Replay};
use super::{HeaderFormatter, LineFormatter, Metadata, StdStream};

/// A line longer than this is written in pieces, so output that never ends a line (a
/// progress bar, binary data) cannot grow without bound.
pub(super) const MAX_LINE: usize = 64 * 1024;

/// There is one per [`Mux`](super::Mux) at most, since every console writes to this
/// process's own stdout and stderr.
pub enum Console<M: Metadata> {
    /// Each line is written as soon as it is complete, to the stream it came from.
    Live { formatter: LineFormatter<M> },
    /// Each capture's output is kept until it finishes, then written in one go.
    Grouped {
        buffer: GroupBuffer,
        layout: Layout<M>,
    },
}

pub enum Layout<M: Metadata> {
    /// One block: the header to stderr, then the capture's stdout, then its stderr.
    Block { header: HeaderFormatter<M> },
    /// One JSON object on one line of stdout.
    JsonLine,
}

/// Holds the unfinished end of a stream, so a line split across reads is emitted whole.
#[derive(Default)]
pub(super) struct LineSplitter {
    pending: Vec<u8>,
}

impl LineSplitter {
    pub(super) fn push(
        &mut self,
        bytes: &[u8],
        mut emit: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        self.pending.extend_from_slice(bytes);
        let mut start = 0;
        while let Some(offset) = self.pending[start..].iter().position(|&b| b == b'\n') {
            emit(&self.pending[start..start + offset])?;
            start += offset + 1;
        }
        while self.pending.len() - start > MAX_LINE {
            emit(&self.pending[start..start + MAX_LINE])?;
            start += MAX_LINE;
        }
        self.pending.drain(..start);
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        mut emit: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let rest = std::mem::take(&mut self.pending);
        emit(&rest)
    }
}

#[derive(Serialize)]
#[serde(bound = "")]
struct JsonLineRecord<'a, M: Metadata> {
    name: &'a str,
    start: &'a M::Start,
    end: &'a M::End,
    stdout: Cow<'a, str>,
    stderr: Cow<'a, str>,
}

fn format_line<M: Metadata>(
    formatter: &LineFormatter<M>,
    name: &str,
    start: &M::Start,
    stream: StdStream,
    line: &[u8],
) -> Option<Vec<u8>> {
    let mut formatted = formatter(name, start, stream, line)?;
    if formatted.last() != Some(&b'\n') {
        formatted.push(b'\n');
    }
    Some(formatted)
}

pub(super) fn print_line<M: Metadata>(
    formatter: &LineFormatter<M>,
    name: &str,
    start: &M::Start,
    stream: StdStream,
    line: &[u8],
) -> io::Result<()> {
    let Some(formatted) = format_line::<M>(formatter, name, start, stream, line) else {
        return Ok(());
    };
    match stream {
        StdStream::Stdout => write_flushed(&mut io::stdout().lock(), &formatted),
        StdStream::Stderr => write_flushed(&mut io::stderr().lock(), &formatted),
    }
}

fn write_flushed(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}

pub(super) fn write_block(
    header: &[u8],
    out: &mut Replay,
    err: &mut Replay,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    stderr.write_all(header)?;
    stderr.flush()?;
    copy_ending_line(out, stdout)?;
    copy_ending_line(err, stderr)
}

/// Ends a body left mid-line, so it cannot run into the stream written after it.
fn copy_ending_line(body: &mut Replay, writer: &mut dyn Write) -> io::Result<()> {
    body.copy_to(writer)?;
    if body.ends_mid_line() {
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

pub(super) fn json_line_record<M: Metadata>(
    name: &str,
    start: &M::Start,
    end: &M::End,
    stdout: &[u8],
    stderr: &[u8],
) -> io::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(&JsonLineRecord::<M> {
        name,
        start,
        end,
        stdout: String::from_utf8_lossy(stdout),
        stderr: String::from_utf8_lossy(stderr),
    })?;
    line.push(b'\n');
    Ok(line)
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct TestMeta;

    impl Metadata for TestMeta {
        type Start = String;
        type End = i32;
    }

    fn lines(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut splitter = LineSplitter::default();
        let mut emitted = Vec::new();
        for chunk in chunks {
            splitter
                .push(chunk, |line| {
                    emitted.push(line.to_vec());
                    Ok(())
                })
                .unwrap();
        }
        splitter
            .finish(|line| {
                emitted.push(line.to_vec());
                Ok(())
            })
            .unwrap();
        emitted
    }

    #[test]
    fn complete_lines_are_emitted_without_their_newline() {
        assert_eq!(lines(&[b"one\ntwo\n"]), [b"one".to_vec(), b"two".to_vec()]);
    }

    #[test]
    fn a_line_split_across_reads_is_emitted_whole() {
        assert_eq!(
            lines(&[b"hel", b"lo\nwor", b"ld\n"]),
            [b"hello".to_vec(), b"world".to_vec()]
        );
    }

    #[test]
    fn an_unterminated_last_line_is_emitted_at_the_end() {
        assert_eq!(
            lines(&[b"done\nno newline"]),
            [b"done".to_vec(), b"no newline".to_vec()]
        );
    }

    #[test]
    fn a_line_past_the_limit_is_emitted_in_pieces() {
        let long = vec![b'x'; MAX_LINE * 2 + 5];

        let emitted = lines(&[&long]);

        let sizes: Vec<usize> = emitted.iter().map(Vec::len).collect();
        assert_eq!(sizes, [MAX_LINE, MAX_LINE, 5]);
    }

    #[test]
    fn a_line_filling_the_limit_exactly_is_not_followed_by_an_empty_one() {
        for pieces in [1, 2] {
            let long = vec![b'x'; MAX_LINE * pieces];

            let emitted = lines(&[&long, b"\n"]);

            let sizes: Vec<usize> = emitted.iter().map(Vec::len).collect();
            assert_eq!(sizes, vec![MAX_LINE; pieces], "{pieces} x MAX_LINE");
        }
    }

    #[test]
    fn bytes_that_are_not_utf8_pass_through() {
        assert_eq!(lines(&[b"\xff\xfe\n"]), [b"\xff\xfe".to_vec()]);
    }

    #[test]
    fn a_formatted_line_always_ends_in_one_newline() {
        let tag: LineFormatter<TestMeta> =
            Arc::new(|name, _, _, line| Some([name.as_bytes(), b"\t", line].concat()));
        let own_newline: LineFormatter<TestMeta> =
            Arc::new(|_, _, _, line| Some([line, b"\n"].concat()));
        let start = String::new();

        assert_eq!(
            format_line::<TestMeta>(&tag, "alpha", &start, StdStream::Stdout, b"hi").unwrap(),
            b"alpha\thi\n"
        );
        assert_eq!(
            format_line::<TestMeta>(&own_newline, "alpha", &start, StdStream::Stdout, b"hi")
                .unwrap(),
            b"hi\n",
            "a formatter's own newline is not doubled"
        );
    }

    #[test]
    fn a_line_the_format_declines_is_dropped() {
        let drop_all: LineFormatter<TestMeta> = Arc::new(|_, _, _, _| None);

        assert!(
            format_line::<TestMeta>(&drop_all, "alpha", &String::new(), StdStream::Stderr, b"hi")
                .is_none()
        );
    }

    #[test]
    fn a_blank_line_passed_through_stays_a_blank_line() {
        let plain: LineFormatter<TestMeta> = Arc::new(|_, _, _, line| Some(line.to_vec()));

        assert_eq!(
            format_line::<TestMeta>(&plain, "alpha", &String::new(), StdStream::Stdout, b"")
                .unwrap(),
            b"\n"
        );
    }

    #[test]
    fn a_block_is_its_header_then_stdout_then_stderr() {
        let mut out = Replay::in_memory();
        let mut err = Replay::in_memory();
        out.write_all(b"result\n").unwrap();
        err.write_all(b"warning\n").unwrap();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());

        write_block(
            b"==> alpha <==\n",
            &mut out,
            &mut err,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        assert_eq!(stdout, b"result\n");
        assert_eq!(stderr, b"==> alpha <==\nwarning\n");
    }

    #[test]
    fn a_group_body_left_mid_line_is_ended() {
        let mut out = Replay::in_memory();
        let mut err = Replay::in_memory();
        out.write_all(b"no newline").unwrap();
        err.write_all(b"warning\n").unwrap();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());

        write_block(b"", &mut out, &mut err, &mut stdout, &mut stderr).unwrap();

        assert_eq!(stdout, b"no newline\n");
        assert_eq!(
            stderr, b"warning\n",
            "a body already ending a line is untouched"
        );
    }

    #[test]
    fn a_json_line_is_one_line_with_start_and_end_nested() {
        let record = json_line_record::<TestMeta>(
            "alpha",
            &"admin".to_string(),
            &2,
            b"line one\nline two\n",
            b"\xff",
        )
        .unwrap();

        assert_eq!(record.iter().filter(|&&b| b == b'\n').count(), 1);
        assert_eq!(record.last(), Some(&b'\n'));
        let value: serde_json::Value = serde_json::from_slice(&record).unwrap();
        assert_eq!(value["name"], "alpha");
        assert_eq!(value["start"], "admin");
        assert_eq!(value["end"], 2);
        assert_eq!(value["stdout"], "line one\nline two\n");
        assert_eq!(value["stderr"], "\u{fffd}", "invalid UTF-8 is replaced");
    }
}
