mod cache;
mod sign;

use crate::aws_sso::AwsSsoManagerError;
use aws_config::Region;
use aws_sdk_ssooidc::config::Credentials;
use cache::CacheManagerInputs;
use chrono::TimeDelta;
use std::path::Path;

pub struct ExecEksInputs<'a> {
    pub account: &'a str,
    pub role: &'a str,
    pub cluster: &'a str,
    pub region: Region,
    pub eks_cache_dir: Option<&'a Path>,
    pub config_dir: &'a Path,
    pub expiry: Option<TimeDelta>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error resolving SSO credentials: {0}")]
    AwsSso(Box<AwsSsoManagerError>),
    #[error("EKS auth signing error: {0}")]
    EksRequestSign(#[from] sign::Error),
    #[error("Invalid or missing cache error: {0}")]
    Cache(#[from] std::io::Error),
    #[error("Invalid credential json: {0}")]
    Serde(#[from] serde_json::Error),
}

impl From<AwsSsoManagerError> for Error {
    fn from(value: AwsSsoManagerError) -> Self {
        Self::AwsSso(Box::new(value))
    }
}

pub type Result = std::result::Result<(), Error>;

pub async fn exec_eks<F>(mut credential_resolver: F, exec_inputs: ExecEksInputs<'_>) -> Result
where
    F: AsyncFnMut() -> std::result::Result<Credentials, AwsSsoManagerError>,
{
    let cache_manager = cache::CacheManager::new(&CacheManagerInputs {
        account_id: exec_inputs.account,
        role: exec_inputs.role,
        cluster: exec_inputs.cluster,
        region: &exec_inputs.region,
        cache_dir: &exec_inputs
            .eks_cache_dir
            .unwrap_or(exec_inputs.config_dir)
            .join("eks"),
    });

    let exec_creds = if let Some(hit) = cache_manager.resolve_cache_hit() {
        hit
    } else {
        let credentials = credential_resolver().await?;

        let k8s_creds = sign::generate_eks_credentials(
            &credentials,
            &exec_inputs.region,
            exec_inputs.cluster,
            exec_inputs.expiry.as_ref(),
        )?;

        let string_creds = serde_json::to_string(&k8s_creds)?;
        cache_manager.cache_credentials(&string_creds)?;
        string_creds
    };

    println!("{}", exec_creds);

    Ok(())
}
