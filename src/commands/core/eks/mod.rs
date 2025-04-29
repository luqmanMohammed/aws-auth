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

#[derive(Debug)]
pub enum Error {
    AwsSso(AwsSsoManagerError),
    EksRequestSign(sign::Error),
    Cache(std::io::Error),
    Serde(serde_json::Error),
}

impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EksRequestSign(err) => writeln!(f, "Eks auth signing error: {}", err),
            Error::Cache(err) => writeln!(f, "Invalid or missing cache error: {}", err),
            Error::Serde(err) => writeln!(f, "Invalid credential json: {}", err),
            Error::AwsSso(err) => writeln!(f, "Error resolving SSO credentials: {}", err),
        }
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
        let credentials = credential_resolver().await.map_err(Error::AwsSso)?;

        let k8s_creds = sign::generate_eks_credentials(
            &credentials,
            &exec_inputs.region,
            exec_inputs.cluster,
            exec_inputs.expiry.as_ref(),
        )
        .map_err(Error::EksRequestSign)?;

        let string_creds = serde_json::to_string(&k8s_creds).map_err(Error::Serde)?;
        cache_manager
            .cache_credentials(&string_creds)
            .map_err(Error::Cache)?;
        string_creds
    };

    println!("{}", exec_creds);

    Ok(())
}
