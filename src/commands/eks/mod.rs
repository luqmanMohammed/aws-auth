mod cache;
mod sign;

use crate::credential_providers::{
    provide_credentials, ProvideCredentials, ProvideCredentialsInput,
};
use aws_config::Region;
use cache::CacheManagerInputs;
use chrono::TimeDelta;
use std::path::PathBuf;

pub struct ExecEksInputs {
    pub cluster: String,
    pub region: Region,
    pub eks_cache_dir: Option<PathBuf>,
    pub expiry: Option<TimeDelta>,
}

#[derive(Debug)]
pub enum Error<PE>
where
    PE: std::fmt::Debug + std::error::Error,
{
    EksRequestSign(sign::Error),
    Cache(std::io::Error),
    Provider(PE),
    Serde(serde_json::Error),
}

impl<PE: std::error::Error> std::error::Error for Error<PE> {}
impl<PE: std::error::Error> std::fmt::Display for Error<PE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EksRequestSign(err) => writeln!(f, "Eks auth signing error: {}", err),
            Error::Cache(err) => writeln!(f, "Invalid or missing cache error: {}", err),
            Error::Serde(err) => writeln!(f, "Invalid credential json: {}", err),
            Error::Provider(err) => {
                writeln!(f, "Error generating AWS auth credentials: {}", err)
            }
        }
    }
}

pub type Result<PE> = std::result::Result<(), Error<PE>>;

pub async fn exec_eks<P: ProvideCredentials>(
    credential_provider: P,
    provider_inputs: &ProvideCredentialsInput,
    exec_inputs: ExecEksInputs,
) -> Result<P::Error> {
    let cache_manager = cache::CacheManager::new(&CacheManagerInputs {
        account_id: &provider_inputs.account,
        role: &provider_inputs.role,
        cluster: &exec_inputs.cluster,
        region: &exec_inputs.region,
        cache_dir: &exec_inputs
            .eks_cache_dir
            .as_deref()
            .unwrap_or(&provider_inputs.config_dir)
            .join("eks"),
    });

    let exec_creds = if let Some(hit) = cache_manager.resolve_cache_hit() {
        hit
    } else {
        let credentials = provide_credentials(credential_provider, provider_inputs)
            .await
            .map_err(Error::Provider)?;

        let k8s_creds = sign::generate_eks_credentials(
            &credentials,
            &exec_inputs.region,
            &exec_inputs.cluster,
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
