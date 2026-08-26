mod auth;
pub mod cache;
pub mod config;
mod types;

use std::path::Path;

use crate::utils::lock::DecayingJsonCounterLockProvider;
use auth::AuthManager;
use aws_config::Region;
use cache::{CacheRefMut, mono_json::MonoJsonCacheManager};
use config::UnverifiedSsoConfig;

pub type CacheManager = MonoJsonCacheManager;
pub type CacheManagerError = cache::mono_json::Error;
pub type ConfigError = config::Error;
pub type LockProvider = DecayingJsonCounterLockProvider;
pub type LockProviderError = std::io::Error;
pub type AwsSsoManager<'a> = AuthManager<'a, CacheManager, LockProvider>;
pub type AwsSsoManagerError = auth::Error<CacheManagerError, LockProviderError>;

pub const CREATE_TOKEN_LOCK_NAME: &str = "aws-sso-create-token-lock";

fn build_aws_sso_manager<'a>(
    cache_manager: impl Into<CacheRefMut<'a, CacheManager>>,
    config_dir: &Path,
    handle_cache: bool,
) -> Result<AwsSsoManager<'a>, ConfigError> {
    let config =
        UnverifiedSsoConfig::from_config_file(&config_dir.join("config.json"))?.verify()?;

    let create_token_retry_threshold = config.create_token_retry_threshold();
    let lock_provider = (create_token_retry_threshold != 0).then(|| {
        LockProvider::new(
            config_dir,
            CREATE_TOKEN_LOCK_NAME,
            create_token_retry_threshold,
            config.create_token_lock_decay(),
        )
    });

    Ok(AwsSsoManager::new(
        cache_manager,
        config.start_url(),
        Region::new(config.sso_region().to_string()),
        config.initial_delay(),
        config.max_attempts(),
        config.retry_interval(),
        None,
        handle_cache,
        config.no_browser(),
        lock_provider,
    ))
}

pub fn build_sso_mgr_cached<'a>(
    config_dir: &Path,
    cache_dir: Option<&Path>,
) -> Result<AwsSsoManager<'a>, ConfigError> {
    let cache_manager = MonoJsonCacheManager::new(cache_dir.unwrap_or(config_dir));
    build_aws_sso_manager(cache_manager, config_dir, true)
}

pub fn build_sso_mgr_manual<'a>(
    cache_manager: &'a mut CacheManager,
    config_dir: &Path,
) -> Result<AwsSsoManager<'a>, ConfigError> {
    build_aws_sso_manager(cache_manager, config_dir, false)
}
