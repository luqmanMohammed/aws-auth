mod exec;

use regex::Regex;
use std::borrow::Cow;
use std::vec;

use crate::{
    alias_providers::{self, AliasProviderError, ProvideAliases},
    aws_sso::{
        build_sso_mgr_manual, cache::ManageCache, AwsSsoManagerError, CacheManager,
        CacheManagerError,
    },
    cmd::Batch,
    utils::resolve_config_dir,
};

#[derive(Debug)]
pub enum Error {
    CacheError(CacheManagerError),
    AwsSso(AwsSsoManagerError),
    MissingRequiredArg(String),
    AliasProvider(AliasProviderError),
}

impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::CacheError(err) => writeln!(f, "Cache error: {}", err),
            Error::AwsSso(error) => todo!(),
            Error::MissingRequiredArg(_) => todo!(),
            Error::AliasProvider(error) => todo!(),
        }
    }
}

pub async fn exec_batch(subcommand: Batch) -> Result<(), Error> {
    match subcommand {
        Batch::Exec {
            batch_common,
            arguments,
        } => {
            let config_dir = resolve_config_dir(batch_common.config_dir.as_deref());
            let cache_dir = batch_common.sso_cache_dir.as_deref().unwrap_or(&config_dir);
            let mut cache_manager = CacheManager::new(cache_dir);
            let mut alias_provider = alias_providers::build_alias_provider(&config_dir);
            cache_manager.load_cache().map_err(Error::CacheError)?;
            let mut sso_manager = build_sso_mgr_manual(&mut cache_manager, &config_dir);

            cache_manager.commit().map_err(Error::CacheError)?;
        }
    }

    Ok(())
}
