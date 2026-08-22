use crate::aws_sso::config::AwsSsoConfig;
use crate::aws_sso::{
    CREATE_TOKEN_LOCK_NAME, ConfigError, DEFAULT_CREATE_TOKEN_LOCK_THRESHOLD, LockProvider,
    LockProviderError,
};
use crate::utils::lock::CounterLockProvider;
use crate::utils::resolve_config_dir;
use std::path::Path;

const LOCK_NAMES: [&str; 1] = [CREATE_TOKEN_LOCK_NAME];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error loading config: {0}")]
    Config(#[from] ConfigError),
    #[error("Error accessing lock: {0}")]
    Lock(#[from] LockProviderError),
}

pub fn exec_unlock(config_dir: Option<&Path>) -> Result<(), Error> {
    let config_dir = resolve_config_dir(config_dir)?;

    let config = AwsSsoConfig::load_config(&config_dir.join("config.json"))?;

    for lock_name in LOCK_NAMES {
        let mut lock_provider = LockProvider::new(
            &config_dir,
            lock_name,
            config
                .create_token_retry_threshold
                .unwrap_or(DEFAULT_CREATE_TOKEN_LOCK_THRESHOLD),
            None,
        );
        if let Err(err) = lock_provider.load_lock() {
            if err.kind() == std::io::ErrorKind::NotFound {
                eprintln!("INFO: Locking is not enabled.");
                continue;
            } else {
                return Err(Error::Lock(err));
            }
        }
        if lock_provider.get_lock().is_locked() {
            lock_provider.get_lock_mut().reset();
            lock_provider.save_lock()?;
            eprintln!("INFO: Lock has been reset.");
        } else {
            eprintln!("INFO: Lock is not set.");
        }
    }

    Ok(())
}
