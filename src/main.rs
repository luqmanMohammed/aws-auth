#![warn(unused_extern_crates)]

mod alias_providers;
mod cmd;
mod commands;
mod common;
mod credential_providers;
mod utils;
mod aws_sso;

use alias_providers::build_alias_provider;
use aws_config::Region;
use chrono::Duration;
use clap::Parser;
use cmd::{Cli, Commands};
use commands::{
    alias::exec_alias,
    eks::{self, ExecEksInputs},
    eval::{self, ExecEvalInputs},
    exec::{self, ExecExecInputs},
    init::{self, ExecInitInputs},
    sso::exec_sso,
};
use credential_providers::{build_credential_provider, ProvideCredentialsInput};
use std::error::Error;
use std::path::Path;

fn error_to_string(error: impl Error) -> String {
    error.to_string()
}

fn setup_providers(
    config_dir: &Path,
) -> Result<
    (
        credential_providers::CredentialProvider,
        alias_providers::AliasProvider,
    ),
    String,
> {
    let credential_provider = build_credential_provider(config_dir).map_err(error_to_string)?;
    let alias_provider = build_alias_provider(config_dir);
    Ok((credential_provider, alias_provider))
}

fn build_credential_provider_inputs(
    config_dir: &Path,
    common: &cmd::CommonArgs,
    mut alias_provider: alias_providers::AliasProvider,
) -> Result<ProvideCredentialsInput, String> {
    let assume_identifier =
        utils::resolve_assume_identifier(&mut alias_provider, common).map_err(error_to_string)?;
    Ok(ProvideCredentialsInput {
        account: assume_identifier.account.to_string(),
        role: assume_identifier.role.to_string(),
        ignore_cache: common.ignore_cache,
        config_dir: config_dir.to_path_buf(),
        cache_dir: common.sso_cache_dir.clone(),
        refresh_sts_token: common.refresh_sts_token,
    })
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init {
            sso_start_url,
            sso_region,
            max_attempts,
            initial_delay_secounds,
            retry_interval_secounds,
            config_dir,
            recreate,
        } => {
            init::exec_init(ExecInitInputs {
                config_dir,
                recreate,
                sso_start_url,
                sso_region,
                max_attempts,
                initial_delay: initial_delay_secounds.map(std::time::Duration::from_secs),
                retry_interval: retry_interval_secounds.map(std::time::Duration::from_secs),
            })
            .map_err(error_to_string)?;
        }
        Commands::Eks {
            common,
            cluster,
            eks_cache_dir,
            eks_expiry_seconds,
        } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let (credential_provider, alias_provider) = setup_providers(&config_dir)?;
            let provider_inputs =
                build_credential_provider_inputs(&config_dir, &common, alias_provider)?;
            eks::exec_eks(
                credential_provider,
                &provider_inputs,
                ExecEksInputs {
                    cluster,
                    eks_cache_dir,
                    region: Region::new(common.region),
                    expiry: eks_expiry_seconds.map(|v| Duration::seconds(v as i64)),
                },
            )
            .await
            .map_err(error_to_string)?;
        }
        Commands::Eval { common } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let (credential_provider, alias_provider) = setup_providers(&config_dir)?;
            let provider_inputs =
                build_credential_provider_inputs(&config_dir, &common, alias_provider)?;
            eval::exec_eval(
                credential_provider,
                &provider_inputs,
                ExecEvalInputs {
                    region: Region::new(common.region),
                },
            )
            .await
            .map_err(error_to_string)?;
        }
        Commands::Exec { common, arguments } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let (credential_provider, alias_provider) = setup_providers(&config_dir)?;
            let provider_inputs =
                build_credential_provider_inputs(&config_dir, &common, alias_provider)?;
            exec::exec_exec(
                credential_provider,
                &provider_inputs,
                ExecExecInputs {
                    region: Region::new(common.region),
                    arguments,
                },
            )
            .await
            .map_err(error_to_string)?;
        }
        Commands::Alias { subcommand } => exec_alias(subcommand).map_err(error_to_string)?,
        Commands::Sso { subcommand } => exec_sso(subcommand).await.map_err(error_to_string)?,
    }
    Ok(())
}
