#![warn(unused_extern_crates)]

mod cmd;
mod commands;
mod credential_providers;
mod utils;

use aws_config::Region;
use chrono::Duration;
use clap::Parser;
use cmd::{Cli, Commands};
use commands::{
    eks::{self, ExecEksInputs},
    eval::{self, ExecEvalInputs},
    exec::{self, ExecExecInputs},
    init::{self, ExecInitInputs},
};
use credential_providers::{build_credential_provider, ProvideCredentialsInput};
use std::error::Error;

fn error_to_string(error: impl Error) -> String {
    error.to_string()
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
            let credential_provider =
                build_credential_provider(&config_dir).map_err(error_to_string)?;
            eks::exec_eks(
                credential_provider,
                &ProvideCredentialsInput {
                    account: common.account,
                    role: common.role,
                    ignore_cache: common.ignore_cache,
                    config_dir,
                    cache_dir: common.cache_dir,
                    refresh_sts_token: common.refresh_sts_token,
                },
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
            let credential_provider =
                build_credential_provider(&config_dir).map_err(error_to_string)?;
            eval::exec_eval(
                credential_provider,
                &ProvideCredentialsInput {
                    account: common.account,
                    role: common.role,
                    ignore_cache: common.ignore_cache,
                    config_dir,
                    cache_dir: common.cache_dir,
                    refresh_sts_token: common.refresh_sts_token,
                },
                ExecEvalInputs {
                    region: Region::new(common.region),
                },
            )
            .await
            .map_err(error_to_string)?;
        }
        Commands::Exec { common, arguments } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let credential_provider =
                build_credential_provider(&config_dir).map_err(error_to_string)?;
            exec::exec_exec(
                credential_provider,
                &ProvideCredentialsInput {
                    account: common.account,
                    role: common.role,
                    ignore_cache: common.ignore_cache,
                    config_dir,
                    cache_dir: common.cache_dir,
                    refresh_sts_token: common.refresh_sts_token,
                },
                ExecExecInputs {
                    region: Region::new(common.region),
                    arguments,
                },
            )
            .await
            .map_err(error_to_string)?;
        }
    }
    Ok(())
}
