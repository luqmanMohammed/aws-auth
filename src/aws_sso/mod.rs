mod api;
mod auth;
pub mod cache;
pub mod config;
mod directory;
mod types;

use std::num::NonZeroU64;
use std::path::Path;

use crate::utils::lockout::Lockout;
use api::LazySdkAdapter;
use auth::AuthManager;
use aws_config::Region;
use cache::CacheRefMut;
use config::{AwsSsoConfig, UnverifiedSsoConfig};
use directory::DirectoryCache;
pub use directory::ListingCache;

pub type CacheManager = cache::CacheStore;
pub type CacheManagerError = cache::Error;
pub type ConfigError = config::Error;
pub type AwsSsoManager<'a> = AuthManager<'a, CacheManager, LazySdkAdapter>;
pub type AwsSsoManagerError = auth::Error<CacheManagerError>;

pub const CREATE_TOKEN_LOCK_NAME: &str = "aws-sso-create-token-lock";

pub fn load_config(config_dir: &Path) -> Result<AwsSsoConfig, ConfigError> {
    UnverifiedSsoConfig::from_config_file(&config_dir.join("config.json"))?.verify()
}

fn build_aws_sso_manager<'a>(
    cache_manager: impl Into<CacheRefMut<'a, CacheManager>>,
    config: &AwsSsoConfig,
    config_dir: &Path,
    cache_dir: &Path,
    handle_cache: bool,
) -> AwsSsoManager<'a> {
    let lockout = NonZeroU64::new(config.create_token_retry_threshold()).map(|threshold| {
        Lockout::new(
            config_dir,
            CREATE_TOKEN_LOCK_NAME,
            threshold,
            config.create_token_lock_decay(),
        )
    });

    let manager = AwsSsoManager::new(
        LazySdkAdapter::new(Region::new(config.sso_region().to_string())),
        cache_manager,
        config.start_url(),
        config.retry_interval(),
        None,
        handle_cache,
        config.no_browser(),
        lockout,
    );
    match config.account_cache_ttl() {
        Some(ttl) => {
            manager.with_directory_cache(DirectoryCache::new(cache_dir, config.start_url(), ttl))
        }
        None => manager,
    }
}

pub fn build_sso_mgr_cached<'a>(
    config_dir: &Path,
    cache_dir: Option<&Path>,
) -> Result<AwsSsoManager<'a>, ConfigError> {
    let cache_dir = cache_dir.unwrap_or(config_dir);
    let config = load_config(config_dir)?;
    let cache_manager = CacheManager::from_config(&config, cache_dir);
    Ok(build_aws_sso_manager(
        cache_manager,
        &config,
        config_dir,
        cache_dir,
        true,
    ))
}

pub fn build_sso_mgr_manual<'a>(
    cache_manager: &'a mut CacheManager,
    config: &AwsSsoConfig,
    config_dir: &Path,
    cache_dir: &Path,
) -> AwsSsoManager<'a> {
    build_aws_sso_manager(cache_manager, config, config_dir, cache_dir, false)
}
