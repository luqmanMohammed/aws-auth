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
};
use credential_providers::{
    aws_sso::{config::AwsSsoConfig, AwsSsoCredentialProvider},
    ProvideCredentialsInput,
};
use std::error::Error;

fn error_to_string(error: impl Error) -> String {
    error.to_string()
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();

    let credential_provider: AwsSsoCredentialProvider =
        AwsSsoConfig::load_config(cli.config_path.as_deref())
            .map_err(error_to_string)?
            .into();

    let provider_input = ProvideCredentialsInput {
        account_id: cli.account_id,
        role: cli.role,
        ignore_cache: cli.ignore_cache,
        cache_dir: cli.cache_dir,
    };

    match cli.command {
        Commands::Eks {
            cluster,
            region,
            eks_cache_dir,
            eks_expiry_seconds,
        } => eks::exec_eks(
            credential_provider,
            &provider_input,
            ExecEksInputs {
                cluster,
                region: Region::new(region),
                eks_cache_dir,
                expiry: eks_expiry_seconds.map(|v| Duration::seconds(v as i64)),
            },
        )
        .await
        .map_err(error_to_string)?,
        Commands::Eval { region } => eval::exec_eval(
            credential_provider,
            provider_input,
            ExecEvalInputs {
                region: Region::new(region),
            },
        )
        .await
        .map_err(error_to_string)?,
    }
    Ok(())
}
