use std::env;
use std::path::{Path, PathBuf};

pub fn resolve_awssso_home(home_path: Option<&Path>) -> PathBuf {
    home_path.map_or_else(
        || {
            let home_dir = home::home_dir().unwrap_or_else(env::temp_dir);
            home_dir.join(".aws-sso-eks-auth")
        },
        PathBuf::from,
    )
}
