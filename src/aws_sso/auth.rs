use crate::aws_sso::cache::CacheManager;
use crate::aws_sso::types::ClientInformation;
use aws_config::{AppName, BehaviorVersion, Region, SdkConfig};
use aws_sdk_sso::operation::get_role_credentials::GetRoleCredentialsError;
use aws_sdk_sso::operation::list_account_roles::ListAccountRolesError;
use aws_sdk_sso::operation::list_accounts::ListAccountsError;
use aws_sdk_sso::types::{AccountInfo, RoleInfo};
use aws_sdk_sso::Client as SsoClient;
use aws_sdk_ssooidc::operation::create_token::CreateTokenError;
use aws_sdk_ssooidc::operation::register_client::RegisterClientError;
use aws_sdk_ssooidc::operation::start_device_authorization::StartDeviceAuthorizationError;
use aws_sdk_ssooidc::{config::Credentials, Client as OidcClient};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_runtime_api::http::Response;
use chrono::{DateTime, Duration, Utc};
use std::thread;
use std::time::UNIX_EPOCH;

const OIDC_APP_NAME: &str = "aws-auth";
const OIDC_CLIENT_TYPE: &str = "public";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_CREATE_TOKEN_INITIAL_DELAY: Duration = Duration::seconds(10);
const DEFAULT_CREATE_TOKEN_RETRY_INTERVAL: Duration = Duration::seconds(5);
const DEFAULT_CREATE_TOKEN_MAX_ATTEMPTS: usize = 10;
const EXPECT_MESSAGE: &str = "Should be present, caller pub function assume_role asures it";

#[derive(Debug)]
pub enum Error<CE: 'static + std::error::Error + std::fmt::Debug> {
    OidcRegisterClient(SdkError<RegisterClientError, Response>),
    OidcStartDeviceAuthorization(SdkError<StartDeviceAuthorizationError, Response>),
    OidcWebBrowserApprove(std::io::Error),
    OidcCreateToken(SdkError<CreateTokenError, Response>),
    OidcTokenRefreshFailed(SdkError<CreateTokenError, Response>),
    SsoGetRoleCredentials(SdkError<GetRoleCredentialsError, Response>),
    OidcListAccounts(SdkError<ListAccountsError, Response>),
    OidcListAccountRoles(SdkError<ListAccountRolesError, Response>),
    Cache(CE),
}

impl<CE: 'static + std::error::Error + std::fmt::Debug> std::fmt::Display for Error<CE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::OidcRegisterClient(err) => writeln!(f, "Oidc Register Client Error: {}", err),
            Error::OidcStartDeviceAuthorization(err) => {
                writeln!(f, "Oidc Start Device Authorization Error: {}", err)
            }
            Error::OidcWebBrowserApprove(err) => {
                writeln!(f, "Oidc Web Browser Approve Error: {}", err)
            }
            Error::OidcCreateToken(err) => writeln!(f, "Oidc Create Token Error: {}", err),
            Error::OidcTokenRefreshFailed(err) => {
                writeln!(f, "Oidc Token Refresh Failed Error: {}", err)
            }
            Error::SsoGetRoleCredentials(err) => {
                writeln!(f, "Sso GetRole Credentials Error: {}", err)
            }
            Error::Cache(err) => writeln!(f, "Cache Error: {}", err),
            Error::OidcListAccounts(err) => {
                writeln!(f, "Oidc List Accounts Error: {}", err)
            }
            Error::OidcListAccountRoles(err) => {
                writeln!(f, "Oidc List Account Roles Error: {}", err)
            }
        }
    }
}

impl<CE: 'static + std::error::Error + std::fmt::Debug> std::error::Error for Error<CE> {}

type Result<T, CE> = std::result::Result<T, Error<CE>>;

pub struct AuthManager<C>
where
    C: 'static + CacheManager,
{
    oidc_client: OidcClient,
    sso_client: SsoClient,
    cache_manager: C,
    start_url: String,
    initial_delay: Duration,
    max_attempts: usize,
    retry_interval: Duration,

    client_info: ClientInformation,
    code_writer: Box<dyn std::io::Write + 'static>,
    handle_cache: bool,
}

impl<C> AuthManager<C>
where
    C: 'static + CacheManager,
    C::Error: 'static + std::error::Error + std::fmt::Debug,
{
    /// TODO: Refactor into a input type
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache_manager: C,
        start_url: impl Into<String>,
        sso_region: Region,
        initial_delay: Option<Duration>,
        max_attempts: Option<usize>,
        retry_interval: Option<Duration>,
        code_writer: Option<Box<dyn std::io::Write + 'static>>,
        handle_cache: bool,
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
            start_url: start_url.into(),
            initial_delay: initial_delay.unwrap_or(DEFAULT_CREATE_TOKEN_INITIAL_DELAY),
            max_attempts: max_attempts.unwrap_or(DEFAULT_CREATE_TOKEN_MAX_ATTEMPTS),
            retry_interval: retry_interval.unwrap_or(DEFAULT_CREATE_TOKEN_RETRY_INTERVAL),
            client_info: ClientInformation::default(),
            code_writer: match code_writer {
                Some(cw) => cw,
                None => Box::new(std::io::stderr()),
            },
            handle_cache,
        }
    }

    async fn prepare_sso_and_resolve<T, F>(
        &mut self,
        resolver: F,
        ignore_cache: bool,
    ) -> Result<T, C::Error>
    where
        F: AsyncFnOnce(&mut Self) -> Result<T, C::Error>,
    {
        if self.handle_cache {
            self.load_cache(ignore_cache);
        }
        if self.client_info.client_id.is_none() || self.client_info.client_secret.is_none() {
            self.register_client().await?;
            self.client_info.access_token = None;
            self.client_info.refresh_token = None;
        }
        if self.client_info.access_token.is_none() && self.client_info.refresh_token.is_some() {
            self.refresh_access_token().await?;
            self.cache_manager.clear_sessions();
        } else if self.client_info.access_token.is_none() {
            self.create_access_token().await?;
            self.cache_manager.clear_sessions();
        }
        let result = resolver(self).await;
        if result.is_ok() {
            self.cache_manager.set_client_info(self.client_info.clone());
            if self.handle_cache {
                self.cache_manager.commit().map_err(Error::Cache)?;
            }
        }
        result
    }

    pub async fn list_accounts(
        &mut self,
        ignore_cache: bool,
    ) -> Result<Vec<AccountInfo>, C::Error> {
        self.prepare_sso_and_resolve(
            async |auth| {
                let access_token = auth
                    .client_info
                    .access_token
                    .as_deref()
                    .expect(EXPECT_MESSAGE);

                let accounts = auth
                    .sso_client
                    .list_accounts()
                    .access_token(access_token)
                    .into_paginator()
                    .send()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .await
                    .map_err(Error::OidcListAccounts)?
                    .into_iter()
                    .filter_map(|res| res.account_list)
                    .flatten()
                    .collect();

                Ok(accounts)
            },
            ignore_cache,
        )
        .await
    }

    pub async fn list_account_roles(
        &mut self,
        account_id: &str,
        ignore_cache: bool,
    ) -> Result<Vec<RoleInfo>, C::Error> {
        self.prepare_sso_and_resolve(
            async |auth| {
                let access_token = auth
                    .client_info
                    .access_token
                    .as_deref()
                    .expect(EXPECT_MESSAGE);
                let roles = auth
                    .sso_client
                    .list_account_roles()
                    .account_id(account_id)
                    .access_token(access_token)
                    .into_paginator()
                    .send()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .await
                    .map_err(Error::OidcListAccountRoles)?
                    .into_iter()
                    .filter_map(|res| res.role_list)
                    .flatten()
                    .collect();
                Ok(roles)
            },
            ignore_cache,
        )
        .await
    }

    pub async fn assume_role(
        &mut self,
        account_id: &str,
        role_name: &str,
        refresh_sts_token: bool,
        ignore_cache: bool,
    ) -> Result<Credentials, C::Error> {
        self.prepare_sso_and_resolve(
            async |auth| {
                let credentials = if refresh_sts_token {
                    auth.resolve_credentials(role_name, account_id).await?
                } else if let Some(cached_credentials) =
                    auth.cache_manager.get_session(account_id, role_name)
                {
                    Credentials::from(cached_credentials.clone())
                } else {
                    auth.resolve_credentials(role_name, account_id).await?
                };
                auth.cache_manager
                    .set_session(account_id, role_name, credentials.clone());
                Ok(credentials)
            },
            ignore_cache,
        )
        .await
    }

    // pub async fn assume_role(
    //     &mut self,
    //     account_id: &str,
    //     role_name: &str,
    //     refresh_sts_token: bool,
    // ) -> Result<Credentials, C::Error> {
    //     self.prepare_sso().await?;
    //     let credentials = if refresh_sts_token {
    //         self.resolve_credentials(role_name, account_id).await?
    //     } else if let Some(cached_credentials) =
    //         self.cache_manager.get_session(account_id, role_name)
    //     {
    //         Credentials::from(cached_credentials.clone())
    //     } else {
    //         self.resolve_credentials(role_name, account_id).await?
    //     };
    //     self.cache_manager
    //         .set_session(account_id, role_name, credentials.clone());
    //     self.commit_cache()?;
    //     Ok(credentials)
    // }

    fn load_cache(&mut self, ignore_cache: bool) {
        if self.cache_manager.load_cache().is_err()
            || !self.cache_manager.is_valid(&self.start_url)
            || ignore_cache
        {
            self.client_info.client_id = None;
            self.client_info.client_secret = None;
        } else {
            self.client_info = self.cache_manager.get_computed_client_info();
        }
        self.client_info.start_url = Some(self.start_url.clone());
    }

    async fn register_client(&mut self) -> Result<(), C::Error> {
        let register_client = self
            .oidc_client
            .register_client()
            .client_name(OIDC_APP_NAME)
            .client_type(OIDC_CLIENT_TYPE)
            .send()
            .await
            .map_err(Error::OidcRegisterClient)?;

        self.client_info.client_id = register_client.client_id;
        self.client_info.client_secret = register_client.client_secret;
        self.client_info.client_secret_expires_at =
            DateTime::from_timestamp(register_client.client_secret_expires_at, 0);

        Ok(())
    }

    async fn create_access_token(&mut self) -> Result<(), C::Error> {
        let device_auth = self
            .oidc_client
            .start_device_authorization()
            .client_id(self.client_info.client_id.as_deref().expect(EXPECT_MESSAGE))
            .client_secret(
                self.client_info
                    .client_secret
                    .as_deref()
                    .expect(EXPECT_MESSAGE),
            )
            .start_url(&self.start_url)
            .send()
            .await
            .map_err(Error::OidcStartDeviceAuthorization)?;

        let _ = writeln!(
            self.code_writer,
            "User Code: {}",
            device_auth.user_code.as_deref().expect(
                "Should be present. StartDeviceAuthorization fails fast in case of an error"
            )
        );

        webbrowser::open(
            device_auth
                .verification_uri_complete
                .as_deref()
                .expect("verification_uri should be present"),
        )
        .map_err(Error::OidcWebBrowserApprove)?;

        thread::sleep(self.initial_delay.to_std().unwrap());

        let device_interval = Duration::seconds(device_auth.interval as i64);
        let interval = if self.retry_interval < device_interval {
            device_interval
        } else {
            self.retry_interval
        };
        let mut attempts = 0;
        let create_token = loop {
            match self
                .oidc_client
                .create_token()
                .client_id(self.client_info.client_id.as_deref().expect(EXPECT_MESSAGE))
                .client_secret(
                    self.client_info
                        .client_secret
                        .as_deref()
                        .expect(EXPECT_MESSAGE),
                )
                .grant_type(GRANT_TYPE)
                .device_code(device_auth.device_code.as_deref().expect(EXPECT_MESSAGE))
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
        .map_err(Error::OidcCreateToken)?;

        self.client_info.access_token = create_token.access_token;
        self.client_info.refresh_token = create_token.refresh_token;
        self.client_info.access_token_expires_at =
            Some(Utc::now() + Duration::seconds(create_token.expires_in as i64));
        Ok(())
    }

    async fn refresh_access_token(&mut self) -> Result<(), C::Error> {
        let create_token = self
            .oidc_client
            .create_token()
            .client_id(self.client_info.client_id.as_deref().expect(EXPECT_MESSAGE))
            .client_secret(
                self.client_info
                    .client_secret
                    .as_deref()
                    .expect(EXPECT_MESSAGE),
            )
            .grant_type("refresh_token")
            .refresh_token(
                self.client_info
                    .refresh_token
                    .as_deref()
                    .expect(EXPECT_MESSAGE),
            )
            .send()
            .await
            .map_err(Error::OidcTokenRefreshFailed)?;
        self.client_info.access_token = create_token.access_token;
        self.client_info.refresh_token = create_token.refresh_token;
        self.client_info.access_token_expires_at =
            Some(Utc::now() + Duration::seconds(create_token.expires_in as i64));
        Ok(())
    }

    async fn resolve_credentials(
        &self,
        role_name: &str,
        account_id: &str,
    ) -> Result<Credentials, C::Error> {
        let credentials = self
            .sso_client
            .get_role_credentials()
            .role_name(role_name)
            .account_id(account_id)
            .access_token(
                self.client_info
                    .access_token
                    .as_deref()
                    .expect(EXPECT_MESSAGE),
            )
            .send()
            .await
            .map_err(Error::SsoGetRoleCredentials)?
            .role_credentials
            .expect("Exit early if GetRoleCredentials fails, role credentials should be present");

        Ok(Credentials::new(
            credentials
                .access_key_id
                .expect("Should be present, Succesfull GetRoleCredentials assures it"),
            credentials
                .secret_access_key
                .expect("Should be present, Succesfull GetRoleCredentials assures it"),
            credentials.session_token,
            Some(
                UNIX_EPOCH
                    + std::time::Duration::from_millis(credentials.expiration.try_into().unwrap()),
            ),
            "role-credentials",
        ))
    }
}
