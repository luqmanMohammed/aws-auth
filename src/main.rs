#![warn(unused_extern_crates)]

mod alias_providers;
mod aws_sso;
mod cmd;
mod commands;
mod common;
mod utils;

use clap::Parser;
use cmd::{Cli, Commands};
use commands::{
    alias::exec_alias,
    batch::exec_batch,
    core::exec_core_commands,
    init::{self, ExecInitInputs},
    logout::exec_logout,
    sso::exec_sso,
    unlock::exec_unlock,
};

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
            create_token_retry_threshold,
            update
        } => {
            init::exec_init(ExecInitInputs {
                config_dir,
                recreate,
                sso_start_url,
                sso_region,
                max_attempts,
                initial_delay: initial_delay_secounds.map(std::time::Duration::from_secs),
                retry_interval: retry_interval_secounds.map(std::time::Duration::from_secs),
                create_token_retry_threshold,
                update
            })
            .map_err(error_to_string)?;
        }
        Commands::Core(command) => exec_core_commands(&command)
            .await
            .map_err(error_to_string)?,
        Commands::Alias { subcommand } => exec_alias(subcommand).map_err(error_to_string)?,
        Commands::Sso { subcommand } => exec_sso(subcommand).await.map_err(error_to_string)?,
        Commands::Batch { subcommand } => exec_batch(subcommand).await.map_err(error_to_string)?,
        Commands::Unlock { config_dir } => {
            exec_unlock(config_dir.as_deref()).map_err(error_to_string)?
        }
        Commands::Logout {
            config_dir,
            cache_dir,
        } => exec_logout(config_dir.as_deref(), cache_dir.as_deref())
            .await
            .map_err(error_to_string)?,
    }
    Ok(())
}
