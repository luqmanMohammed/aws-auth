use crate::aws_sso::config::UnverifiedSsoConfig;
use crate::aws_sso::{CREATE_TOKEN_LOCK_NAME, ConfigError, LockProvider, LockProviderError};
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

    let config =
        UnverifiedSsoConfig::from_config_file(&config_dir.join("config.json"))?.verify()?;

    for lock_name in LOCK_NAMES {
        let mut lock_provider = LockProvider::new(
            &config_dir,
            lock_name,
            config.create_token_retry_threshold(),
            None,
        );
        if let Err(err) = lock_provider.load_lock() {
            match err.kind() {
                std::io::ErrorKind::NotFound => {
                    eprintln!("INFO: Locking is not enabled.");
                    continue;
                }
                std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => {
                    lock_provider.discard_lock()?;
                    eprintln!("INFO: Lock could not be read ({err}) and has been removed.");
                    continue;
                }
                _ => return Err(Error::Lock(err)),
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
