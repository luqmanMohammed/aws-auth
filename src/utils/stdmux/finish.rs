use std::fs::File;
use std::io::{self, Write};

use super::console;
use super::replay::Replay;
use super::{Console, Delivery, Layout, LogPaths, Metadata, Mux};

/// What one stream of a capture left behind once nothing more can be written to it.
#[derive(Default)]
pub(super) struct FinishedStream {
    pub(super) replay: Option<Replay>,
    pub(super) read_error: Option<io::Error>,
    pub(super) log_error: Option<io::Error>,
    pub(super) console_error: Option<io::Error>,
}

impl FinishedStream {
    /// For a stream whose producer wrote straight to `file`.
    pub(super) fn direct(file: File, replayed: bool) -> io::Result<Self> {
        Ok(Self {
            replay: if replayed {
                Some(Replay::from_file(file)?)
            } else {
                None
            },
            ..Self::default()
        })
    }
}

fn write_grouped<M: Metadata>(
    layout: &Layout<M>,
    name: &str,
    start: &M::Start,
    end: &M::End,
    out: &mut Replay,
    err: &mut Replay,
) -> io::Result<()> {
    match layout {
        Layout::Block { header } => {
            let header = header(name, start, end);
            // Anything holding both locks takes stdout first, so two of them cannot deadlock.
            let mut stdout = io::stdout().lock();
            let mut stderr = io::stderr().lock();
            console::write_block(&header, out, err, &mut stdout, &mut stderr)
        }
        Layout::JsonLine => {
            let record = console::json_line_record::<M>(
                name,
                start,
                end,
                &out.read_all()?,
                &err.read_all()?,
            )?;
            let mut stdout = io::stdout().lock();
            stdout.write_all(&record)?;
            stdout.flush()
        }
    }
}

/// Writes a grouped console's output, then reports what went wrong on the way.
pub(super) fn deliver<M: Metadata>(
    mux: &Mux<M>,
    name: &str,
    start: &M::Start,
    end: &M::End,
    logs: Option<LogPaths>,
    mut out: FinishedStream,
    mut err: FinishedStream,
) -> io::Result<Delivery> {
    if let Some(error) = out.read_error.take().or_else(|| err.read_error.take()) {
        return Err(error);
    }
    let log_error = out.log_error.take().or_else(|| err.log_error.take());
    let mut console_error = out
        .console_error
        .take()
        .or_else(|| err.console_error.take());
    if console_error.is_none()
        && let Some(Console::Grouped { layout, .. }) = &mux.console
    {
        let (Some(out_replay), Some(err_replay)) = (&mut out.replay, &mut err.replay) else {
            return Err(io::Error::other("grouped output missing"));
        };
        console_error = write_grouped(layout, name, start, end, out_replay, err_replay).err();
    }
    Ok(Delivery {
        logs,
        log_error,
        console_error,
    })
}
