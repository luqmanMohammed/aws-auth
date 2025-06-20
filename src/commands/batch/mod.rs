mod exec;

use std::collections::HashMap;

use crate::utils::worker::ThreadPool;
use aws_sdk_ssooidc::config::Credentials;
use exec::ExecJob;
use regex::Regex;
use std::sync::Arc;

use crate::{
    alias_providers::{self, AliasProviderError, ProvideAliases},
    aws_sso::{
        build_sso_mgr_manual, cache::ManageCache, AwsSsoManagerError, CacheManager,
        CacheManagerError,
    },
    cmd::Batch,
    elog,
    utils::resolve_config_dir,
};

#[derive(Debug)]
pub enum Error {
    Cache(CacheManagerError),
    AwsSso(Box<AwsSsoManagerError>),
    MissingRequiredArg(String),
    AliasProvider(AliasProviderError),
    Regex(regex::Error),
    ValidationFailed(String),
}

impl From<AwsSsoManagerError> for Error {
    fn from(value: AwsSsoManagerError) -> Self {
        Self::AwsSso(Box::new(value))
    }
}

impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Cache(err) => write!(f, "Cache error: {}", err),
            Error::AwsSso(err) => write!(f, "Error getting credentials from AWS SSO: {}", err),
            Error::MissingRequiredArg(err) => write!(f, "Provide arguments: {}", err),
            Error::AliasProvider(err) => write!(f, "Error getting alias: {}", err),
            Error::Regex(err) => write!(f, "Invalid regex provided: {}", err),
            Error::ValidationFailed(err) => write!(f, "Command Input validation failed: {}", err),
        }
    }
}

pub async fn exec_batch(subcommand: Batch) -> Result<(), Error> {
    match &subcommand {
        Batch::Exec { arguments, .. } => {
            exec::ExecJob::validate(arguments)
                .map_err(|err| Error::ValidationFailed(err.to_string()))?;
        }
    }

    let batch_common = subcommand.get_common_args();
    let config_dir = resolve_config_dir(batch_common.config_dir.as_deref());
    let cache_dir = batch_common.sso_cache_dir.as_deref().unwrap_or(&config_dir);
    let mut cache_manager = CacheManager::new(cache_dir);
    let mut alias_provider = alias_providers::build_alias_provider(&config_dir);
    let mut sso_manager = build_sso_mgr_manual(&mut cache_manager, &config_dir);
    sso_manager.load_cache(batch_common.ignore_cache);

    let grouped_possible_assumes: Vec<(String, String)> = if let Some(ref aliases) =
        batch_common.aliases
    {
        alias_provider
            .load_aliases()
            .map_err(Error::AliasProvider)?;
        aliases
            .iter()
            .filter_map(|alias| {
                if let Ok(Some(assume_identity)) = alias_provider.get_alias(alias) {
                    Some((
                        assume_identity.account.to_string(),
                        assume_identity.role.to_string(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    } else {
        let role_order = batch_common
            .role_order
            .as_deref()
            .ok_or(Error::MissingRequiredArg(
                "Missing required input role_oder".to_string(),
            ))?;
        if let Some(account_ids) = &batch_common.account_ids {
            account_ids
                .iter()
                .flat_map(|account_id| {
                    role_order
                        .iter()
                        .map(move |role| (account_id.to_string(), role.to_string()))
                })
                .collect::<Vec<_>>()
        } else if let Some(account_name_regex) = &batch_common.account_filter_regex {
            let regex = Regex::new(&format!("^{}", account_name_regex)).map_err(Error::Regex)?;

            sso_manager
                .list_accounts(batch_common.ignore_cache)
                .await?
                .into_iter()
                .filter(|ai| {
                    ai.account_name.as_ref().is_some()
                        && regex.is_match(ai.account_name().unwrap())
                        && ai.account_id().is_some()
                })
                .flat_map(|ai| {
                    let account_id = ai.account_id().unwrap().to_string();
                    role_order
                        .iter()
                        .map(move |role| (account_id.clone(), role.to_string()))
                })
                .collect::<Vec<_>>()
        } else {
            sso_manager
                .list_accounts(batch_common.ignore_cache)
                .await?
                .into_iter()
                .filter(|ai| ai.account_id().is_some())
                .flat_map(|ai| {
                    let account_id = ai.account_id().unwrap().to_string();
                    role_order
                        .iter()
                        .map(move |role| (account_id.clone(), role.to_string()))
                })
                .collect::<Vec<_>>()
        }
    };

    let mut credentials_map: HashMap<String, Credentials> = HashMap::new();
    for (account_id, role_name) in grouped_possible_assumes {
        if credentials_map.contains_key(&account_id) {
            continue;
        }
        match sso_manager
            .assume_role(&account_id, &role_name, false, batch_common.ignore_cache)
            .await
        {
            Ok(credentials) => {
                elog!(batch_common.debug, "Succesffuly resolved credentials for account {account_id} using the {role_name} role");
                credentials_map.insert(account_id.clone(), credentials);
            }
            Err(err) => {
                if let AwsSsoManagerError::SsoGetRoleCredentials(_) = err {
                    elog!(batch_common.debug, "Unauthorized to resolve credentials for account {account_id} using the {role_name} role");
                } else {
                    Err(Error::AwsSso(Box::new(err)))?;
                }
            }
        }
    }

    cache_manager.commit().map_err(Error::Cache)?;

    match subcommand {
        Batch::Exec {
            arguments,
            suppress_output,
            output_dir,
            batch_common,
        } => {
            let arguments: Arc<[String]> = Arc::from(arguments.into_boxed_slice());
            let _ = &arguments
                .first()
                .ok_or(Error::MissingRequiredArg("Missing program".to_string()))?;
            let worker_pool: ThreadPool<ExecJob> =
                ThreadPool::new(batch_common.parallel, batch_common.debug);
            let output_dir = output_dir.map(Arc::new);
            let region = Arc::new(batch_common.region);
            for (account_id, credentials) in credentials_map {
                worker_pool.execute(ExecJob {
                    account_id,
                    arguments: arguments.clone(),
                    output_base_path: output_dir.clone(),
                    credentials,
                    suppress_output,
                    region: region.clone(),
                });
            }
            let result = worker_pool.wait();
            elog!(batch_common.debug, "{result:?}");
        }
    }

    Ok(())
}
