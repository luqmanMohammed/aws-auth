pub mod aws_sso;
use std::path::{Path, PathBuf};

use crate::credential_providers::aws_sso::config::{AwsSsoConfig, Error as AwsSsoConfigError};
use crate::credential_providers::aws_sso::AwsSsoCredentialProvider;
use aws_sdk_sso::config::Credentials;

pub struct ProvideCredentialsInput {
    pub account: String,
    pub role: String,
    pub ignore_cache: bool,
    pub config_dir: PathBuf,
    pub cache_dir: Option<PathBuf>,
    pub refresh_sts_token: bool,
}

pub trait ProvideCredentials {
    type Error: std::error::Error + Sync + Send;
    async fn provide_credentials(
        self,
        input: &ProvideCredentialsInput,
    ) -> Result<Credentials, Self::Error>;
}

pub async fn provide_credentials<T: ProvideCredentials>(
    provider: T,
    input: &ProvideCredentialsInput,
) -> Result<Credentials, T::Error> {
    provider.provide_credentials(input).await
}

pub fn build_credential_provider(
    config_dir: &Path,
) -> Result<AwsSsoCredentialProvider, AwsSsoConfigError> {
    let credential_provider: AwsSsoCredentialProvider =
        AwsSsoConfig::load_config(&config_dir.join("config.json"))?.into();
    Ok(credential_provider)
}
