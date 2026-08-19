use std::fs::File;
use std::io;
use std::path::Path;

#[cfg(unix)]
const FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;

pub fn create_file(path: &Path) -> io::Result<File> {
    let mut options = File::options();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(FILE_MODE);
    }

    let file = options.open(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // `mode` above only applies to files this call created, so a file left behind by
        // an older version keeps its old permissions unless tightened explicitly.
        file.set_permissions(std::fs::Permissions::from_mode(FILE_MODE))?;
    }

    Ok(file)
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
