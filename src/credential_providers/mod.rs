pub mod aws_sso;
use std::path::PathBuf;

use aws_sdk_sso::config::Credentials;

pub struct ProvideCredentialsInput {
    pub account: String,
    pub role: String,
    pub ignore_cache: bool,
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
