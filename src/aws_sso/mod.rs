mod auth;
pub mod cache;
pub mod config;
mod types;

use std::path::Path;

use crate::utils::lock::DecayingJsonCounterLockProvider;
use auth::AuthManager;
use aws_config::Region;
use cache::{mono_json::MonoJsonCacheManager, CacheRefMut};
use chrono::Duration;
use config::AwsSsoConfig;

pub type CacheManager = MonoJsonCacheManager;
pub type CacheManagerError = cache::mono_json::Error;
pub type LockProvider = DecayingJsonCounterLockProvider;
pub type LockProviderError = std::io::Error;
pub type AwsSsoManager<'a> = AuthManager<'a, CacheManager, LockProvider>;
pub type AwsSsoManagerError = auth::Error<CacheManagerError, LockProviderError>;

pub const DEFAULT_CREATE_TOKEN_LOCK_THRESHOLD: u64 = 5;
pub const DEFAULT_CREATE_TOKEN_LOCK_DECAY: chrono::Duration = chrono::Duration::seconds(2 * 3600);

fn build_aws_sso_manager<'a>(
    cache_manager: impl Into<CacheRefMut<'a, CacheManager>>,
    config_dir: &Path,
    handle_cache: bool,
) -> AwsSsoManager<'a> {
    let config =
        AwsSsoConfig::load_config(&config_dir.join("config.json")).expect("Config should be valid");
    let initial_delay = config
        .initial_delay
        .map(|d| Duration::from_std(d).expect("Config should be valid"));

    let retry_interval = config
        .retry_interval
        .map(|d| Duration::from_std(d).expect("Config should be valid"));

    let create_token_lock_decay = match config.create_token_lock_decay {
        Some(td) if td.num_seconds() == 0 => None,
        Some(td) => Some(td),
        None => Some(DEFAULT_CREATE_TOKEN_LOCK_DECAY),
    };

    let lock_provider = config
        .create_token_retry_threshold
        .filter(|&threshold| threshold != 0)
        .or(Some(DEFAULT_CREATE_TOKEN_LOCK_THRESHOLD))
        .map(|threshold| {
            LockProvider::new(
                config_dir,
                "aws-sso-create-token-lock",
                threshold,
                create_token_lock_decay,
            )
        });

    AwsSsoManager::new(
        cache_manager,
        config.start_url,
        Region::new(config.sso_reigon),
        initial_delay,
        config.max_attempts,
        retry_interval,
        None,
        handle_cache,
        lock_provider,
    )
}

pub fn build_sso_mgr_cached<'a>(config_dir: &Path, cache_dir: Option<&Path>) -> AwsSsoManager<'a> {
    let cache_manager = MonoJsonCacheManager::new(cache_dir.unwrap_or(config_dir));
    build_aws_sso_manager(cache_manager, config_dir, true)
}

pub fn build_sso_mgr_manual<'a>(
    cache_manager: &'a mut CacheManager,
    config_dir: &Path,
) -> AwsSsoManager<'a> {
    build_aws_sso_manager(cache_manager, config_dir, false)
}
