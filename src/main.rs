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
};
use credential_providers::{
    aws_sso::{config::AwsSsoConfig, AwsSsoCredentialProvider},
    ProvideCredentialsInput,
};
use std::{error::Error, path::Path};

fn error_to_string(error: impl Error) -> String {
    error.to_string()
}

fn build_credential_provider(
    config_path: Option<&Path>,
) -> Result<AwsSsoCredentialProvider, String> {
    let credential_provider: AwsSsoCredentialProvider = AwsSsoConfig::load_config(config_path)
        .map_err(error_to_string)?
        .into();
    Ok(credential_provider)
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Eks {
            common,
            cluster,
            eks_cache_dir,
            eks_expiry_seconds,
        } => {
            let credential_provider = build_credential_provider(common.config_path.as_deref())?;
            eks::exec_eks(
                credential_provider,
                &ProvideCredentialsInput {
                    account: common.account,
                    role: common.role,
                    ignore_cache: common.ignore_cache,
                    cache_dir: common.cache_dir,
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
            let credential_provider = build_credential_provider(common.config_path.as_deref())?;
            eval::exec_eval(
                credential_provider,
                &ProvideCredentialsInput {
                    account: common.account,
                    role: common.role,
                    ignore_cache: common.ignore_cache,
                    cache_dir: common.cache_dir,
                },
                ExecEvalInputs {
                    region: Region::new(common.region),
                },
            )
            .await
            .map_err(error_to_string)?;
        }
        Commands::Exec { common, arguments } => {
            let credential_provider = build_credential_provider(common.config_path.as_deref())?;
            exec::exec_exec(
                credential_provider,
                &ProvideCredentialsInput {
                    account: common.account,
                    role: common.role,
                    ignore_cache: common.ignore_cache,
                    cache_dir: common.cache_dir,
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
