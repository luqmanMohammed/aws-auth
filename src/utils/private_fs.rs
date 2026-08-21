use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
const FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;

fn create_file(path: &Path) -> io::Result<File> {
    let mut options = File::options();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(FILE_MODE);
    }

    options.open(path)
}

fn temp_path(path: &Path) -> io::Result<PathBuf> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{path:?} has no parent directory"),
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{path:?} has no file name"),
        )
    })?;
    Ok(dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    )))
}

/// Replaces `path` in one step, so a concurrent reader sees either the old contents or the
/// new ones but never a partial write. The replacement also carries the owner-only mode,
/// which is what tightens files left behind by earlier versions.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    // Resolved first so a path that is a symlink is written through rather than replaced by the
    // rename below, which would otherwise quietly detach it from its target.
    let path = &fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let temp_path = temp_path(path)?;
    let _ = fs::remove_file(&temp_path);

    let write = || -> io::Result<()> {
        let mut file = create_file(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()
    };

    if let Err(err) = write().and_then(|()| fs::rename(&temp_path, path)) {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    Ok(())
}

pub fn create_dir_all(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(DIR_MODE)
            .create(path)
    }

    #[cfg(not(unix))]
    std::fs::create_dir_all(path)
}
