use crate::aws_sso::{build_sso_mgr_cached, AwsSsoManagerError};
use crate::utils::resolve_config_dir;
use std::path::Path;

pub async fn exec_logout(
    config_dir: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Result<(), AwsSsoManagerError> {
    let config_dir = resolve_config_dir(config_dir);
    let sso_mgr = build_sso_mgr_cached(&config_dir, cache_dir);
    sso_mgr.logout().await?;
    println!("INFO: Successfully logged out of all SSO sessions.");
    Ok(())
}
