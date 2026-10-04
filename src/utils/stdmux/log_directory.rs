use std::fs::File;
use std::io;
use std::path::PathBuf;

use super::LogPaths;
use crate::utils::private_fs;

/// An output directory that exists. Every log in it is created owner-only and replaces
/// whatever was at its path, since nothing is known about the output it holds and it has to
/// be assumed sensitive.
pub struct LogDirectory {
    path: PathBuf,
}

impl LogDirectory {
    pub fn create(path: PathBuf) -> io::Result<Self> {
        private_fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub(super) fn open_logs(&self, name: &str) -> io::Result<(File, File, LogPaths)> {
        check_file_name(name)?;
        let paths = LogPaths {
            stdout: self.path.join(format!("{name}-stdout.log")),
            stderr: self.path.join(format!("{name}-stderr.log")),
        };
        let stdout = private_fs::create_replacing(&paths.stdout)?;
        let stderr = private_fs::create_replacing(&paths.stderr)?;
        Ok((stdout, stderr, paths))
    }
}

/// A name becomes part of a file name, so one holding a separator could write outside the
/// directory.
fn check_file_name(name: &str) -> io::Result<()> {
    if name
        .chars()
        .any(|c| std::path::is_separator(c) || c == '\0')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} cannot be used in a log file name"),
        ));
    }
    Ok(())
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;
    use std::fs;
    use std::io::Write;

    #[cfg(unix)]
    use crate::utils::test_support::mode_of;

    fn create(dir: &TempDir) -> LogDirectory {
        LogDirectory::create(dir.path().to_path_buf()).unwrap()
    }

    #[test]
    fn logs_are_named_after_the_id() {
        let dir = TempDir::new("stdmux-log-directory-names");

        let (_, _, paths) = create(&dir).open_logs("alpha").unwrap();

        assert_eq!(paths.stdout, dir.join("alpha-stdout.log"));
        assert_eq!(paths.stderr, dir.join("alpha-stderr.log"));
        assert!(paths.stdout.is_file() && paths.stderr.is_file());
    }

    #[test]
    fn logs_from_an_earlier_run_are_replaced() {
        let dir = TempDir::new("stdmux-log-directory-replace");
        fs::write(dir.join("alpha-stdout.log"), "stale").unwrap();

        let (mut stdout, _, _) = create(&dir).open_logs("alpha").unwrap();
        stdout.write_all(b"fresh").unwrap();

        assert_eq!(fs::read(dir.join("alpha-stdout.log")).unwrap(), b"fresh");
    }

    #[test]
    fn an_id_that_would_leave_the_directory_is_rejected() {
        let dir = TempDir::new("stdmux-log-directory-escape");
        let directory = create(&dir);

        for name in ["../alpha", "a/b", "/etc/alpha", "nul\0"] {
            let err = directory.open_logs(name).expect_err(name);
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{name:?}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn the_directory_and_its_logs_are_owner_only() {
        let dir = TempDir::new("stdmux-log-directory-mode");
        let nested = dir.join("runs/today");

        let directory = LogDirectory::create(nested.clone()).unwrap();
        let (_, _, paths) = directory.open_logs("alpha").unwrap();

        assert_eq!(mode_of(&nested), 0o700);
        assert_eq!(mode_of(&paths.stdout), 0o600);
        assert_eq!(mode_of(&paths.stderr), 0o600);
    }
}
