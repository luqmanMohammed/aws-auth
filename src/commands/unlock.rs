use crate::aws_sso::config::UnverifiedSsoConfig;
use crate::aws_sso::{CREATE_TOKEN_LOCK_NAME, ConfigError};
use crate::utils::lockout::Lockout;
use crate::utils::resolve_config_dir;
use std::num::NonZeroU64;
use std::path::Path;

const LOCK_NAMES: [&str; 1] = [CREATE_TOKEN_LOCK_NAME];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error loading config: {0}")]
    Config(#[from] ConfigError),
    #[error("Error accessing lock: {0}")]
    Lock(#[from] std::io::Error),
}

pub fn exec_unlock(config_dir: Option<&Path>) -> Result<(), Error> {
    let config_dir = resolve_config_dir(config_dir)?;

    let config =
        UnverifiedSsoConfig::from_config_file(&config_dir.join("config.json"))?.verify()?;

    let Some(threshold) = NonZeroU64::new(config.create_token_retry_threshold()) else {
        eprintln!("INFO: Locking is not enabled.");
        return Ok(());
    };

    for lock_name in LOCK_NAMES {
        let mut lockout = Lockout::new(&config_dir, lock_name, threshold, None);
        let mut loaded = lockout.load()?;
        let was_locked = loaded.is_locked();
        loaded.reset();
        loaded.save()?;
        if was_locked {
            eprintln!("INFO: Lock has been reset.");
        } else {
            eprintln!("INFO: Lock is not set.");
        }
    }

    Ok(())
}
