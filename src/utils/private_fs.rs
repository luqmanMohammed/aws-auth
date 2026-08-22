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

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;

    #[cfg(unix)]
    use crate::utils::test_support::mode_of;

    #[test]
    fn writes_a_new_file_with_the_given_contents() {
        let dir = TempDir::new("pf-new");
        let target = dir.join("cache.json");

        write_atomic(&target, b"hello").expect("write should succeed");

        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
    }

    #[test]
    #[cfg(unix)]
    fn new_files_are_owner_only() {
        let dir = TempDir::new("pf-mode");
        let target = dir.join("cache.json");

        write_atomic(&target, b"secret").expect("write should succeed");

        assert_eq!(mode_of(&target), 0o600);
    }

    #[test]
    #[cfg(unix)]
    fn tightens_a_file_left_behind_with_loose_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("pf-tighten");
        let target = dir.join("cache.json");
        fs::write(&target, b"old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(mode_of(&target), 0o644, "precondition");

        write_atomic(&target, b"new").expect("write should succeed");

        assert_eq!(mode_of(&target), 0o600);
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    #[cfg(unix)]
    fn created_directories_are_owner_only() {
        let dir = TempDir::new("pf-dir");
        let nested = dir.join("a/b/c");

        create_dir_all(&nested).expect("create should succeed");

        assert_eq!(mode_of(&nested), 0o700);
        assert_eq!(mode_of(&dir.join("a")), 0o700);
    }

    #[test]
    fn create_dir_all_accepts_an_existing_directory() {
        let dir = TempDir::new("pf-dir-twice");
        let nested = dir.join("again");

        create_dir_all(&nested).expect("first create should succeed");
        create_dir_all(&nested).expect("second create should also succeed");
    }

    #[test]
    fn a_shorter_write_leaves_no_trailing_bytes() {
        let dir = TempDir::new("pf-truncate");
        let target = dir.join("cache.json");

        write_atomic(&target, b"aaaaaaaaaaaaaaaaaaaaaaaa").expect("first write");
        write_atomic(&target, b"bb").expect("second write");

        assert_eq!(fs::read_to_string(&target).unwrap(), "bb");
    }

    #[test]
    fn leaves_no_temporary_file_behind() {
        let dir = TempDir::new("pf-temp");
        let target = dir.join("cache.json");

        write_atomic(&target, b"x").expect("write should succeed");

        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "cache.json")
            .collect();
        assert!(leftovers.is_empty(), "unexpected leftovers: {leftovers:?}");
    }

    #[test]
    #[cfg(unix)]
    fn writes_through_a_symlink_instead_of_replacing_it() {
        let dir = TempDir::new("pf-symlink");
        let real = dir.join("real.json");
        let link = dir.join("link.json");
        fs::write(&real, b"before").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_atomic(&link, b"after").expect("write should succeed");

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink should survive the write"
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "after");
    }

    #[test]
    fn reports_an_error_when_the_directory_is_missing() {
        let dir = TempDir::new("pf-missing");
        let target = dir.join("no-such-dir/cache.json");

        let result = write_atomic(&target, b"x");

        assert!(result.is_err(), "expected a failure, got {result:?}");
    }
}
