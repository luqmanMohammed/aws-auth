mod eks;
mod eval;
mod exec;

use aws_config::Region;
use chrono::Duration;
use eks::ExecEksInputs;
use eval::ExecEvalInputs;
use exec::ExecExecInputs;

use crate::{
    alias_providers,
    aws_sso::{build_sso_mgr_cached, AwsSsoManagerError},
    cmd::CoreCommands,
    utils::{resolve_assume_identifier, resolve_config_dir},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error resolving assume identifier: {0}")]
    AssumeIdResolver(String),
    #[error("Error resolving SSO credentials: {0}")]
    AwsSso(Box<AwsSsoManagerError>),
    #[error("Error executing command: {0}")]
    CmdExec(#[from] exec::Error),
    #[error("Error executing EKS command: {0}")]
    CmdEks(#[from] eks::Error),
}

impl From<AwsSsoManagerError> for Error {
    fn from(value: AwsSsoManagerError) -> Self {
        Self::AwsSso(Box::new(value))
    }
}

pub async fn exec_core_commands(command: &CoreCommands) -> Result<(), Error> {
    let common_args = command.get_common_args();
    let config_dir = resolve_config_dir(common_args.config_dir.as_deref());
    let mut sso_manager = build_sso_mgr_cached(&config_dir, common_args.sso_cache_dir.as_deref());
    let mut alias_provider = alias_providers::build_alias_provider(&config_dir);
    let assume_identity = resolve_assume_identifier(&mut alias_provider, common_args)
        .map_err(|err| Error::AssumeIdResolver(err.to_string()))?;

    let mut credential_resolver = async || {
        sso_manager
            .assume_role(
                assume_identity.account,
                assume_identity.role,
                common_args.refresh_sts_token,
                common_args.ignore_cache,
            )
            .await
    };

    match command {
        CoreCommands::Eks {
            cluster,
            eks_cache_dir,
            eks_expiry_seconds,
            ..
        } => {
            eks::exec_eks(
                credential_resolver,
                ExecEksInputs {
                    account: assume_identity.account,
                    role: assume_identity.role,
                    cluster,
                    region: Region::new(common_args.region.clone()),
                    eks_cache_dir: eks_cache_dir.as_deref(),
                    config_dir: &config_dir,
                    expiry: eks_expiry_seconds.map(|v| Duration::seconds(v as i64)),
                },
            )
            .await?;
        }
        CoreCommands::Eval { output, .. } => {
            let credentials = credential_resolver().await?;
            eval::exec_eval(
                credentials,
                ExecEvalInputs {
                    region: Region::new(common_args.region.clone()),
                    output,
                },
            );
        }
        CoreCommands::Exec { arguments, .. } => {
            let credentials = credential_resolver().await?;
            exec::exec_exec(
                credentials,
                ExecExecInputs {
                    region: Region::new(common_args.region.clone()),
                    arguments: arguments.clone(),
                },
            )
            .await?;
        }
    }
    Ok(())
}
