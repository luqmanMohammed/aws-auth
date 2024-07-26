pub mod aws_cmd;
pub mod aws_sso;

use crate::cmd::Arguments;
use crate::types::K8sExecCredentials;
use aws_config::Region;

pub struct ProvideCredentialsInput {
    account_id: String,
    role: String,
    region: Region,
    cluster: String,
}

impl From<Arguments> for ProvideCredentialsInput {
    fn from(value: Arguments) -> Self {
        Self {
            account_id: value.account,
            role: value.role,
            cluster: value.cluster_name,
            region: Region::new(value.region),
        }
    }
}

pub trait ProvideCredentials {
    type Error: std::error::Error + Sync + Send;
    async fn provide_credentials(
        self,
        input: &ProvideCredentialsInput,
    ) -> Result<K8sExecCredentials, Self::Error>;
}

pub async fn provide_credentials<T: ProvideCredentials>(
    provider: T,
    input: &ProvideCredentialsInput,
) -> Result<K8sExecCredentials, T::Error> {
    provider.provide_credentials(input).await
}
