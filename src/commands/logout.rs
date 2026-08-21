use crate::aws_sso::{AwsSsoManagerError, ConfigError, build_sso_mgr_cached};
use crate::utils::resolve_config_dir;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error loading config: {0}")]
    Config(#[from] ConfigError),
    #[error("Error logging out of SSO sessions: {0}")]
    AwsSso(Box<AwsSsoManagerError>),
}

impl From<AwsSsoManagerError> for Error {
    fn from(value: AwsSsoManagerError) -> Self {
        Self::AwsSso(Box::new(value))
    }
}

pub async fn exec_logout(config_dir: Option<&Path>, cache_dir: Option<&Path>) -> Result<(), Error> {
    let config_dir = resolve_config_dir(config_dir)?;
    let sso_mgr = build_sso_mgr_cached(&config_dir, cache_dir)?;
    sso_mgr.logout().await?;
    eprintln!("INFO: Successfully logged out of all SSO sessions.");
    Ok(())
}
