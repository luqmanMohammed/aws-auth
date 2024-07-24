use super::aws_cache::CacheManager;
use aws_config::{AppName, BehaviorVersion, Region, SdkConfig};
use aws_sdk_sso::operation::get_role_credentials::{
    GetRoleCredentialsError, GetRoleCredentialsOutput,
};
use aws_sdk_sso::types::RoleCredentials;
use aws_sdk_sso::Client as SsoClient;
use aws_sdk_ssooidc::operation::create_token::{CreateTokenError, CreateTokenOutput};
use aws_sdk_ssooidc::operation::register_client::{RegisterClientError, RegisterClientOutput};
use aws_sdk_ssooidc::operation::start_device_authorization::{
    StartDeviceAuthorizationError, StartDeviceAuthorizationOutput,
};
use aws_sdk_ssooidc::{config::Credentials, Client as OidcClient};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_runtime_api::http::Response;
use chrono::{DateTime, Duration};
use std::thread;

const OIDC_APP_NAME: &str = "aws-sso-eks-auth";
const OIDC_CLIENT_TYPE: &str = "public";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_CREATE_TOKEN_INITIAL_DELAY: Duration = Duration::seconds(10);
const DEFAULT_CREATE_TOKEN_RETRY_INTERVAL: Duration = Duration::seconds(5);
const DEFAULT_CREATE_TOKEN_MAX_ATTEMPTS: usize = 10;

enum AwsAuthError<CE: 'static + std::error::Error + std::fmt::Debug> {
    OidcRegisterClientError(SdkError<RegisterClientError, Response>),
    OidcStartDeviceAuthorizationError(SdkError<StartDeviceAuthorizationError, Response>),
    OidcWebBrowserApproveError(std::io::Error),
    OidcCreateTokenError(SdkError<CreateTokenError, Response>),
    OidcTokenRefreshFailed(SdkError<CreateTokenError, Response>),
    SsoGetRoleCredentialsError(SdkError<GetRoleCredentialsError, Response>),
    CacheError(CE),
}

struct AuthBuilder {}

pub struct AwsAuthManager<C>
where
    C: 'static + CacheManager,
{
    oidc_client: OidcClient,
    sso_client: SsoClient,
    cache_manager: C,
    start_url: String,
    initial_deplay: Duration,
    max_attempts: usize,
    retry_interval: Duration,
}

impl<C> AwsAuthManager<C>
where
    C: 'static + CacheManager,
    C::Error: 'static + std::error::Error + std::fmt::Debug,
{
    pub fn new(
        cache_manager: C,
        start_url: String,
        sso_region: Region,
        initial_deplay: Option<Duration>,
        max_attempts: Option<usize>,
        retry_interval: Option<Duration>,
    ) -> Self {
        let sdk_config = SdkConfig::builder()
            .app_name(AppName::new(OIDC_APP_NAME).expect("Const app name should be valid"))
            .behavior_version(BehaviorVersion::latest())
            .region(sso_region)
            .build();
        let oidc_client = OidcClient::new(&sdk_config);
        let sso_client = SsoClient::new(&sdk_config);

        Self {
            oidc_client,
            sso_client,
            cache_manager,
            start_url,
            initial_deplay: initial_deplay.unwrap_or(DEFAULT_CREATE_TOKEN_INITIAL_DELAY),
            max_attempts: max_attempts.unwrap_or(DEFAULT_CREATE_TOKEN_MAX_ATTEMPTS),
            retry_interval: retry_interval.unwrap_or(DEFAULT_CREATE_TOKEN_RETRY_INTERVAL),
        }
    }

    pub async fn assume_role(
        &mut self,
        account_id: &str,
        role_name: &str,
    ) -> Result<Credentials, AwsAuthError<C::Error>> {
        self.cache_manager
            .load_cache()
            .map_err(AwsAuthError::CacheError)?;

        if self.cache_manager.is_valid(&self.start_url) {
        } else {
            let register_client = self.register_oidc_client().await?;
            let start_device_auth = self
                .start_device_auth(
                    register_client.client_id.as_deref().unwrap(),
                    register_client.client_secret.as_deref().unwrap(),
                )
                .await?;

            let create_access_token = self
                .create_access_token(
                    &register_client.client_id.as_deref().unwrap(),
                    &register_client.client_secret.as_deref().unwrap(),
                    &start_device_auth.device_code.as_deref().unwrap(),
                    &Duration::seconds(start_device_auth.interval as i64),
                )
                .await?;

            let role_credentials = self
                .get_credentials_from_access_token(
                    create_access_token.access_token.as_deref().unwrap(),
                    role_name,
                    account_id,
                )
                .await?
                .role_credentials
                .expect("role credentials should be present since its success");

            let creds = from_role_credentials(role_credentials);

            self.cache_manager.set_client_info(
                register_client.client_id.unwrap(),
                register_client.client_secret.unwrap(),
                register_client.client_secret_expires_at,
            );
            self.cache_manager.set_access_token(
                create_access_token.access_token.unwrap(),
                create_access_token.expires_in,
            );
            self.cache_manager
                .set_session(account_id, role_name, creds.clone());
        };

        todo!()
    }

    async fn register_oidc_client(&self) -> Result<RegisterClientOutput, AwsAuthError<C::Error>> {
        self.oidc_client
            .register_client()
            .client_name(OIDC_APP_NAME)
            .client_type(OIDC_CLIENT_TYPE)
            .send()
            .await
            .map_err(AwsAuthError::OidcRegisterClientError)
    }

    async fn start_device_auth(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<StartDeviceAuthorizationOutput, AwsAuthError<C::Error>> {
        let device_auth = self
            .oidc_client
            .start_device_authorization()
            .client_id(client_id)
            .client_secret(client_secret)
            .start_url(&self.start_url)
            .send()
            .await
            .map_err(AwsAuthError::OidcStartDeviceAuthorizationError)?;

        eprintln!("User Code: {}", device_auth.user_code.as_deref().unwrap());

        webbrowser::open(
            device_auth
                .verification_uri
                .as_deref()
                .expect("verification_uri should be present"),
        )
        .map_err(AwsAuthError::OidcWebBrowserApproveError)?;

        thread::sleep(self.initial_deplay.to_std().unwrap());
        Ok(device_auth)
    }

    async fn create_access_token(
        &self,
        client_id: &str,
        client_secret: &str,
        device_code: &str,
        device_interval: &Duration,
    ) -> Result<CreateTokenOutput, AwsAuthError<C::Error>> {
        let interval = if self.retry_interval < *device_interval {
            *device_interval
        } else {
            self.retry_interval
        };
        let mut attempts = 0;
        loop {
            match self
                .oidc_client
                .create_token()
                .client_id(client_id)
                .client_secret(client_secret)
                .grant_type(GRANT_TYPE)
                .device_code(device_code)
                .send()
                .await
            {
                Ok(token) => break Ok(token),
                Err(err) if attempts >= self.max_attempts => break Err(err),
                Err(_) => {
                    thread::sleep(interval.to_std().unwrap());
                    attempts += 1;
                }
            }
        }
        .map_err(AwsAuthError::OidcCreateTokenError)
    }

    async fn refresh_token(
        &self,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<CreateTokenOutput, AwsAuthError<C::Error>> {
        self.oidc_client
            .create_token()
            .client_id(client_id)
            .client_secret(client_secret)
            .grant_type("refresh_token")
            .refresh_token(refresh_token)
            .send()
            .await
            .map_err(AwsAuthError::OidcTokenRefreshFailed)
    }

    async fn get_credentials_from_access_token(
        &self,
        access_token: &str,
        role_name: &str,
        account_id: &str,
    ) -> Result<GetRoleCredentialsOutput, AwsAuthError<C::Error>> {
        self.sso_client
            .get_role_credentials()
            .role_name(role_name)
            .account_id(account_id)
            .access_token(access_token)
            .send()
            .await
            .map_err(AwsAuthError::SsoGetRoleCredentialsError)
    }
}

fn from_role_credentials(role_credentials: RoleCredentials) -> Credentials {
    Credentials::new(
        role_credentials.access_key_id.unwrap(),
        role_credentials.secret_access_key.unwrap(),
        role_credentials.session_token,
        DateTime::from_timestamp(role_credentials.expiration, 0).map(|d| d.into()),
        "role-credentials",
    )
}
