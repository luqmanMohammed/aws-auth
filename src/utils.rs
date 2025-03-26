use std::env;
use std::path::{Path, PathBuf};

pub fn resolve_config_dir(config_dir: Option<&Path>) -> PathBuf {
    config_dir.map_or_else(
        || {
            let home_dir = home::home_dir().unwrap_or_else(env::temp_dir);
            home_dir.join(".aws-auth")
        },
        PathBuf::from,
    )
}
