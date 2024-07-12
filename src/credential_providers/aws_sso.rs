use crate::types::{
    K8sExecCredentials, K8sExecCredentialsStatus, DEFAULT_EXEC_CREDENTIALS_API_VERSION,
    DEFAULT_EXEC_CREDENTIALS_KIND,
};
use aws_config::{AppName, BehaviorVersion, Region};
use aws_sdk_sso::operation::get_role_credentials::GetRoleCredentialsError;
use aws_sdk_ssooidc::config::Credentials;
use aws_sdk_ssooidc::error::SdkError;
use aws_sdk_ssooidc::operation::create_token::CreateTokenError;
use aws_sdk_ssooidc::operation::register_client::RegisterClientError;
use aws_sdk_ssooidc::operation::start_device_authorization::StartDeviceAuthorizationError;
use aws_sigv4::http_request::{
    self, SignableRequest, SignatureLocation, SigningError, SigningParams, SigningSettings,
};
use aws_sigv4::sign;
use aws_smithy_runtime_api::client::identity::Identity;
use aws_smithy_runtime_api::http as smithy_http;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use http::request::Request;
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, SystemTime};

const K8S_AWS_ID_HEADER: &str = "x-k8s-aws-id";
const TOKEN_PREFIX: &str = "k8s-aws-v1";
const OIDC_APP_NAME: &str = "aws-sso-eks-auth";
const OIDC_CLIENT_TYPE: &str = "public";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_EXPIRTY: Duration = Duration::from_secs(860);
const DEFAULT_CREATE_TOKEN_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_CREATE_TOKEN_MAX_RETRIES: u8 = 10;

#[derive(Debug)]
pub enum ProviderAwsSsoError {
    SigningError(SigningError),
    RequestBuildError(http::Error),
    OidcRegisterClientError(SdkError<RegisterClientError, smithy_http::Response>),
    OidcDeviceAuthError(SdkError<StartDeviceAuthorizationError, smithy_http::Response>),
    BrowserError(std::io::Error),
    OidcCreateTokenRetriesExpired(SdkError<CreateTokenError, smithy_http::Response>),
    SsoGetRoleCredentialsError(SdkError<GetRoleCredentialsError, smithy_http::Response>),
}

impl std::fmt::Display for ProviderAwsSsoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderAwsSsoError::SigningError(err) => writeln!(f, "Signing Error: {}", err),
            ProviderAwsSsoError::RequestBuildError(err) => {
                writeln!(f, "Request build Error: {}", err)
            }
            ProviderAwsSsoError::OidcRegisterClientError(err) => {
                writeln!(f, "OIDC Register Client Error: {}", err)
            }
            ProviderAwsSsoError::OidcDeviceAuthError(err) => {
                writeln!(f, "OIDC Device Auth Error: {}", err)
            }
            ProviderAwsSsoError::BrowserError(err) => {
                writeln!(f, "Browser Error: {}", err)
            }
            ProviderAwsSsoError::OidcCreateTokenRetriesExpired(err) => {
                writeln!(
                    f,
                    "Create token failed for all retries. Final retry Error: {}",
                    err
                )
            }
            ProviderAwsSsoError::SsoGetRoleCredentialsError(err) => {
                writeln!(f, "Failed to get Role Credentials: {}", err)
            }
        }
    }
}

impl std::error::Error for ProviderAwsSsoError {}

pub struct AwsSsoCredentialProvider {
    start_url: String,
    sso_region: Region,
    expires_in: Option<Duration>,
    max_retries: Option<usize>,
    interval: Option<Duration>,
}

impl AwsSsoCredentialProvider {
    #[allow(dead_code)]
    pub fn new(
        start_url: String,
        sso_region: Region,
        expires_in: Option<Duration>,
        max_retries: Option<usize>,
        interval: Option<Duration>,
    ) -> Self {
        Self {
            start_url,
            sso_region,
            expires_in,
            max_retries,
            interval,
        }
    }

    pub fn minimal(start_url: String, sso_region: Region) -> Self {
        Self {
            start_url,
            sso_region,
            expires_in: None,
            max_retries: None,
            interval: None,
        }
    }

    async fn create_role_credentials_from_oidc(
        &self,
        account_id: &str,
        role_arn: &str,
    ) -> Result<Credentials, ProviderAwsSsoError> {
        let sdkconfig = aws_config::SdkConfig::builder()
            .app_name(AppName::new(OIDC_APP_NAME).expect("Const app name should be valid"))
            .behavior_version(BehaviorVersion::latest())
            .region(self.sso_region.clone())
            .build();

        let ssooidc_client = aws_sdk_ssooidc::Client::new(&sdkconfig);
        let register_client = ssooidc_client
            .register_client()
            .client_name(OIDC_APP_NAME)
            .client_type(OIDC_CLIENT_TYPE)
            .send()
            .await
            .map_err(ProviderAwsSsoError::OidcRegisterClientError)?;

        let client_id = register_client
            .client_id
            .expect("client_id should be present");
        let client_secret = register_client
            .client_secret
            .expect("client_id should be present");

        let device_auth = ssooidc_client
            .start_device_authorization()
            .client_id(&client_id)
            .client_secret(&client_secret)
            .start_url(&self.start_url)
            .send()
            .await
            .map_err(ProviderAwsSsoError::OidcDeviceAuthError)?;

        let verification_uri = device_auth
            .verification_uri_complete()
            .expect("verification_uri_complete should be present");

        webbrowser::open(verification_uri).map_err(ProviderAwsSsoError::BrowserError)?;

        let device_code = device_auth
            .device_code
            .expect("device code should be present");
        eprintln!(
            "Device Code : {}",
            device_auth.user_code.expect("user code should be present")
        );

        let max_retries = self
            .max_retries
            .unwrap_or(DEFAULT_CREATE_TOKEN_MAX_RETRIES.into());
        let interval = self.interval.unwrap_or(DEFAULT_CREATE_TOKEN_INTERVAL);
        let mut tries = 0;
        let token = loop {
            match ssooidc_client
                .create_token()
                .client_id(&client_id)
                .client_secret(&client_secret)
                .grant_type(GRANT_TYPE)
                .device_code(&device_code)
                .send()
                .await
            {
                Ok(token) => break Ok(token),
                Err(err) if tries >= max_retries => break Err(err),
                Err(_) => {
                    thread::sleep(interval);
                    tries += 1;
                }
            }
        };

        let token = token.map_err(ProviderAwsSsoError::OidcCreateTokenRetriesExpired)?;

        let sso_client = aws_sdk_sso::Client::new(&sdkconfig);
        let credentials = sso_client
            .get_role_credentials()
            .role_name(role_arn)
            .account_id(account_id)
            .access_token(token.access_token.expect("token should be present"))
            .send()
            .await
            .map_err(ProviderAwsSsoError::SsoGetRoleCredentialsError)?
            .role_credentials
            .expect("role credentials should be present");

        let credentials = aws_sdk_sso::config::Credentials::new(
            credentials
                .access_key_id
                .expect("access_key_id should be present"),
            credentials
                .secret_access_key
                .expect("secret_access_key should be present"),
            credentials.session_token,
            None,
            "awsso-oidc",
        );
        Ok(credentials)
    }

    async fn generate_auth_credentials(
        &self,
        credentials: &Credentials,
        region: &Region,
        cluster_name: &str,
    ) -> Result<K8sExecCredentials, ProviderAwsSsoError> {
        let expires_in = self.expires_in.unwrap_or(DEFAULT_EXPIRTY);

        let mut settings = SigningSettings::default();
        settings.expires_in = Some(expires_in);
        settings.signature_location = SignatureLocation::QueryParams;

        let identity = &Identity::from(credentials.to_owned());
        let region = region.to_string();

        let params = sign::v4::SigningParams::builder()
            .identity(identity)
            .region(&region)
            .name("sts")
            .time(SystemTime::now())
            .settings(settings)
            .build()
            .expect("there should not be any build errors");

        let uri = format!(
            "https://sts.{region}.amazonaws.com/?Action=GetCallerIdentity&Version=2011-06-15",
            region = region
        );

        let request = SignableRequest::new(
            "GET",
            &uri,
            vec![(K8S_AWS_ID_HEADER, cluster_name)].into_iter(),
            aws_sigv4::http_request::SignableBody::Bytes(&[]),
        )
        .map_err(ProviderAwsSsoError::SigningError)?;

        let (signing_instruction, _) = http_request::sign(request, &SigningParams::V4(params))
            .map_err(ProviderAwsSsoError::SigningError)?
            .into_parts();

        let mut request = Request::builder()
            .uri(&uri)
            .body(())
            .map_err(ProviderAwsSsoError::RequestBuildError)?;

        signing_instruction.apply_to_request_http1x(&mut request);
        let encoded_url = URL_SAFE.encode(request.uri().to_string().into_bytes());

        Ok(K8sExecCredentials {
            kind: DEFAULT_EXEC_CREDENTIALS_KIND.to_string(),
            api_version: DEFAULT_EXEC_CREDENTIALS_API_VERSION.to_string(),
            spec: HashMap::new(),
            status: K8sExecCredentialsStatus {
                expiration_timestamp: chrono::Utc::now()
                    + chrono::Duration::seconds(expires_in.as_secs().try_into().unwrap()),
                token: format!("{}.{}", TOKEN_PREFIX, encoded_url.trim_end_matches('=')),
            },
        })
    }
}

impl super::ProvideCredentials for AwsSsoCredentialProvider {
    type Error = ProviderAwsSsoError;
    async fn provide_credentials(
        &self,
        input: &super::ProvideCredentialsInput,
    ) -> Result<K8sExecCredentials, Self::Error> {
        let credentials = self
            .create_role_credentials_from_oidc(&input.account_id, &input.role)
            .await?;
        self.generate_auth_credentials(&credentials, &input.region, &input.cluster)
            .await
    }
}
