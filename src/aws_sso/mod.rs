mod auth;
pub mod cache;
pub mod config;
mod types;

use std::path::Path;

use auth::AuthManager;
use aws_config::Region;
use cache::{mono_json::MonoJsonCacheManager, CacheRefMut};
use chrono::Duration;
use config::AwsSsoConfig;

pub type CacheManager = MonoJsonCacheManager;
pub type CacheManagerError = cache::mono_json::Error;
pub type AwsSsoManager<'a> = AuthManager<'a, CacheManager>;
pub type AwsSsoManagerError = auth::Error<CacheManagerError>;

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
    AwsSsoManager::new(
        cache_manager,
        config.start_url,
        Region::new(config.sso_reigon),
        initial_delay,
        config.max_attempts,
        retry_interval,
        None,
        handle_cache,
    )
}

pub fn build_sso_mgr_cached<'a>(
    config_dir: &Path,
    cache_dir: Option<&Path>,
) -> AwsSsoManager<'a> {
    let cache_manager = MonoJsonCacheManager::new(cache_dir.unwrap_or(config_dir));
    build_aws_sso_manager(cache_manager, config_dir, true)
}

pub fn build_sso_mgr_manual<'a>(
    cache_manager: &'a mut CacheManager,
    config_dir: &Path,
) -> AwsSsoManager<'a> {
    build_aws_sso_manager(cache_manager, config_dir, false)
}
