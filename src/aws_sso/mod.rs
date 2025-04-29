mod auth;
mod cache;
pub mod config;
mod types;

use std::path::Path;

use auth::AuthManager;
use aws_config::Region;
use aws_sdk_sso::config::Credentials;
use cache::mono_json::MonoJsonCacheManager;
use chrono::Duration;
use config::AwsSsoConfig;

use crate::utils;

type CacheManager = MonoJsonCacheManager;
type CacheManagerError = cache::mono_json::Error;
pub type AwsSsoManager = AuthManager<CacheManager>;
pub type AwsSsoManagerError = auth::Error<CacheManagerError>;

fn build_aws_sso_manager(
    cache_manager: CacheManager,
    config_dir: &Path,
    handle_cache: bool,
) -> AwsSsoManager {
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

pub fn build_aws_sso_manager_with_cache_handling(
    config_dir: Option<&Path>,
    cache_dir: Option<&Path>,
) -> AwsSsoManager {
    let config_dir = utils::resolve_config_dir(config_dir);
    let cache_manager = MonoJsonCacheManager::new(cache_dir.unwrap_or(&config_dir));
    build_aws_sso_manager(cache_manager, &config_dir, true)
}

pub fn build_aws_sso_manager_with_manual_cache_handling(
    cache_manager: CacheManager,
    config_dir: Option<&Path>,
) -> AwsSsoManager {
    let config_dir = utils::resolve_config_dir(config_dir);
    build_aws_sso_manager(cache_manager, &config_dir, false)
}
