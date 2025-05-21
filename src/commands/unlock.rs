use crate::aws_sso::config::AwsSsoConfig;
use crate::aws_sso::{LockProvider, LockProviderError};
use crate::utils::lock::CounterLockProvider;
use crate::utils::resolve_config_dir;
use std::path::Path;

pub fn exec_unlock(config_dir: Option<&Path>) -> Result<(), LockProviderError> {
    let config_dir = resolve_config_dir(config_dir);
    let config =
        AwsSsoConfig::load_config(&config_dir.join("config.json")).expect("Config should be valid");
    if let Some(threshold) = config.create_token_retry_threshold {
        let mut lock_provider =
            LockProvider::new(&config_dir, "aws-sso-create-token-lock", threshold);
        lock_provider.load_lock()?;
        if lock_provider.get_lock().is_locked() {
            lock_provider.get_lock_mut().reset();
            lock_provider.save_lock()?;
            println!("INFO: Lock has been reset.");
        } else {
            println!("INFO: Lock is not set.");
        }
    } else {
        println!("INFO: Create token upstream lock is not enabled. Use aws-auth init -h for more information.");
    }
    Ok(())
}
