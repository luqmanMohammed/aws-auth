mod exec;

use std::collections::{HashMap, HashSet};

use crate::utils::worker::{JobError, ThreadPool};
use aws_sdk_ssooidc::config::Credentials;
use exec::ExecJob;
use regex::Regex;
use std::sync::Arc;

use crate::{
    alias_providers::{self, AliasProviderError, ProvideAliases},
    aws_sso::{
        AwsSsoManagerError, CacheManager, CacheManagerError, ConfigError, build_sso_mgr_manual,
        cache::ManageCache,
    },
    cmd::Batch,
    elog,
    utils::resolve_config_dir,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cache error: {0}")]
    Cache(#[from] CacheManagerError),
    #[error("Error loading config: {0}")]
    Config(#[from] ConfigError),
    #[error("Error getting credentials from AWS SSO: {0}")]
    AwsSso(Box<AwsSsoManagerError>),
    #[error("Provide arguments: {0}")]
    MissingRequiredArg(String),
    #[error("Error getting alias: {0}")]
    AliasProvider(#[from] AliasProviderError),
    #[error("Invalid regex provided: {0}")]
    Regex(#[from] regex::Error),
    #[error("Command Input validation failed: {0}")]
    ValidationFailed(String),
    #[error("No accounts matched the given account ids, aliases or filter")]
    NoAccountsTargeted,
    #[error("Could not resolve credentials for any of the {0} targeted accounts")]
    NoCredentialsResolved(usize),
    #[error("{failed} of {total} accounts failed")]
    JobsFailed { failed: usize, total: usize },
}

impl From<AwsSsoManagerError> for Error {
    fn from(value: AwsSsoManagerError) -> Self {
        Self::AwsSso(Box::new(value))
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
    let config_dir = resolve_config_dir(batch_common.config_dir.as_deref())?;
    let cache_dir = batch_common.sso_cache_dir.as_deref().unwrap_or(&config_dir);
    let mut cache_manager = CacheManager::new(cache_dir);
    let mut alias_provider = alias_providers::build_alias_provider(&config_dir);
    let mut sso_manager = build_sso_mgr_manual(&mut cache_manager, &config_dir)?;
    sso_manager.load_cache(batch_common.ignore_cache);

    let grouped_possible_assumes: Vec<(String, String)> = if let Some(ref aliases) =
        batch_common.aliases
    {
        alias_provider.load_aliases()?;
        let mut resolved = Vec::with_capacity(aliases.len());
        let mut unknown: Vec<&str> = Vec::new();
        for alias in aliases {
            match alias_provider.get_alias(alias)? {
                Some(assume_identity) => resolved.push((
                    assume_identity.account.to_string(),
                    assume_identity.role.to_string(),
                )),
                None => unknown.push(alias),
            }
        }
        if !unknown.is_empty() {
            eprintln!(
                "WARN: {} of {} aliases did not resolve to an account and were skipped: {}",
                unknown.len(),
                aliases.len(),
                unknown.join(", ")
            );
        }
        resolved
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
            let regex = Regex::new(&format!("^{}", account_name_regex))?;

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

    if grouped_possible_assumes.is_empty() {
        return Err(Error::NoAccountsTargeted);
    }

    let mut credentials_map: HashMap<String, Credentials> = HashMap::new();
    for (account_id, role_name) in &grouped_possible_assumes {
        if credentials_map.contains_key(account_id) {
            continue;
        }
        match sso_manager
            .assume_role(account_id, role_name, false, batch_common.ignore_cache)
            .await
        {
            Ok(credentials) => {
                elog!(
                    batch_common.debug,
                    "Succesffuly resolved credentials for account {account_id} using the {role_name} role"
                );
                credentials_map.insert(account_id.clone(), credentials);
            }
            Err(err) => {
                if let AwsSsoManagerError::SsoGetRoleCredentials(_) = err {
                    elog!(
                        batch_common.debug,
                        "Could not resolve credentials for account {account_id} using the {role_name} role: {err}"
                    );
                } else {
                    Err(Error::AwsSso(Box::new(err)))?;
                }
            }
        }
    }

    // Reported unconditionally: a role_order fallback failing is routine, but an account that
    // resolved under no role at all is silently missing from the run.
    let mut seen = HashSet::new();
    let mut skipped: Vec<&str> = Vec::new();
    for (account_id, _) in &grouped_possible_assumes {
        if !credentials_map.contains_key(account_id) && seen.insert(account_id.as_str()) {
            skipped.push(account_id);
        }
    }
    let targeted = credentials_map.len() + skipped.len();
    if !skipped.is_empty() {
        eprintln!(
            "WARN: Could not resolve credentials for {} of {} accounts: {}",
            skipped.len(),
            targeted,
            skipped.join(", ")
        );
    }
    if credentials_map.is_empty() {
        return Err(Error::NoCredentialsResolved(targeted));
    }

    cache_manager.commit()?;

    match subcommand {
        Batch::Exec {
            arguments,
            suppress_output,
            output_dir,
            fail_fast,
            batch_common,
        } => {
            let arguments: Arc<[String]> = Arc::from(arguments.into_boxed_slice());
            let _ = &arguments
                .first()
                .ok_or(Error::MissingRequiredArg("Missing program".to_string()))?;
            let worker_pool: ThreadPool<ExecJob> =
                ThreadPool::new(batch_common.parallel, batch_common.debug, fail_fast);
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

            let results = worker_pool.wait();
            elog!(batch_common.debug, "{results:?}");

            let total = results.len();
            let mut failed = 0;
            let mut skipped = 0;
            for job in &results {
                match &job.result {
                    Ok(_) => {}
                    Err(JobError::Skipped) => skipped += 1,
                    Err(err) => {
                        failed += 1;
                        eprintln!("WARN: account {} failed: {err}", job.job_id);
                    }
                }
            }
            if skipped > 0 {
                eprintln!("WARN: {skipped} of {total} accounts were skipped after --fail-fast");
            }

            // --fail-fast decides only whether the remaining accounts still run. Making it decide
            // the exit status too would leave that status depending on how many accounts were
            // targeted, which is the one thing a caller in a `set -e` script cannot work around.
            if failed > 0 {
                return Err(Error::JobsFailed { failed, total });
            }
        }
    }

    Ok(())
}
