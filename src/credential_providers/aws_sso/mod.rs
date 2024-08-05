mod auth;
mod cache;
pub mod config;
mod eks;
mod types;

use auth::AuthManager;
use aws_config::Region;
use cache::mono_json::MonoJsonCacheManager;
use chrono::Duration;
use eks::generate_eks_credentials;

use crate::credential_providers::{ProvideCredentials, ProvideCredentialsInput};
use crate::types::K8sExecCredentials;

type CacheManager = MonoJsonCacheManager;
type CacheManagerError = cache::mono_json::Error;

#[derive(Debug)]
pub enum Error {
    AwsAuth(auth::Error<CacheManagerError>),
    EksCreds(eks::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::AwsAuth(err) => writeln!(f, "Failed to generate Aws credentials: {}", err),
            Error::EksCreds(err) => writeln!(f, "Failed to generate Eks credentials: {}", err),
        }
    }
}

impl std::error::Error for Error {}

pub struct AwsSsoCredentialProvider {
    start_url: String,
    sso_region: Region,
    initial_delay: Option<Duration>,
    retry_interval: Option<Duration>,
    max_attempts: Option<usize>,
    expires_in: Option<Duration>,
}

impl AwsSsoCredentialProvider {
    #[allow(dead_code)]
    fn new(
        start_url: String,
        sso_region: Region,
        initial_delay: Option<Duration>,
        retry_interval: Option<Duration>,
        max_attempts: Option<usize>,
        expires_in: Option<Duration>,
    ) -> Self {
        Self {
            start_url,
            sso_region,
            initial_delay,
            retry_interval,
            max_attempts,
            expires_in,
        }
    }
}

impl ProvideCredentials for AwsSsoCredentialProvider {
    type Error = Error;

    async fn provide_credentials(
        self,
        input: &ProvideCredentialsInput,
    ) -> Result<K8sExecCredentials, Self::Error> {
        let cache_manager: CacheManager = MonoJsonCacheManager::new(None);
        let mut auth_manager = AuthManager::new(
            cache_manager,
            self.start_url,
            self.sso_region,
            self.initial_delay,
            self.max_attempts,
            self.retry_interval,
            None,
        );
        let credentials = auth_manager
            .assume_role(&input.account_id, &input.role)
            .await
            .map_err(Error::AwsAuth)?;
        generate_eks_credentials(
            &credentials,
            &input.region,
            &input.cluster,
            self.expires_in.as_ref(),
        )
        .map_err(Error::EksCreds)
    }
}

impl From<config::AwsSsoConfig> for AwsSsoCredentialProvider {
    fn from(value: config::AwsSsoConfig) -> Self {
        Self {
            start_url: value.start_url,
            sso_region: Region::new(value.sso_reigon),
            initial_delay: value
                .initial_delay
                .map(|d| Duration::from_std(d).expect("Config should be valid")),
            retry_interval: value
                .retry_interval
                .map(|d| Duration::from_std(d).expect("Config should be valid")),
            expires_in: value
                .expires_in
                .map(|d| Duration::from_std(d).expect("Config should be valid")),
            max_attempts: value.max_attempts,
        }
    }
}
