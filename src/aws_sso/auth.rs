use super::cache::CacheRefMut;
use crate::aws_sso::cache::ManageCache;
use crate::aws_sso::types::ClientInformation;
use crate::utils::lock::CounterLockProvider;
use aws_config::{AppName, BehaviorVersion, Region, SdkConfig};
use aws_sdk_sso::Client as SsoClient;
use aws_sdk_sso::operation::get_role_credentials::GetRoleCredentialsError;
use aws_sdk_sso::operation::list_account_roles::ListAccountRolesError;
use aws_sdk_sso::operation::list_accounts::ListAccountsError;
use aws_sdk_sso::types::{AccountInfo, RoleInfo};
use aws_sdk_ssooidc::operation::create_token::CreateTokenError;
use aws_sdk_ssooidc::operation::register_client::RegisterClientError;
use aws_sdk_ssooidc::operation::start_device_authorization::StartDeviceAuthorizationError;
use aws_sdk_ssooidc::{Client as OidcClient, config::Credentials};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_runtime_api::http::Response;
use chrono::{DateTime, Duration, Utc};
use std::time::UNIX_EPOCH;

const OIDC_APP_NAME: &str = "aws-auth";
const OIDC_CLIENT_TYPE: &str = "public";
const OIDC_SCOPE: &str = "sso:account:access";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_CREATE_TOKEN_INITIAL_DELAY: Duration = Duration::seconds(10);
const DEFAULT_CREATE_TOKEN_RETRY_INTERVAL: Duration = Duration::seconds(5);
const DEFAULT_CREATE_TOKEN_MAX_ATTEMPTS: usize = 10;
const EXPECT_MESSAGE: &str = "Should be present, caller pub function assume_role asures it";

#[derive(Debug)]
pub enum Error<
    CE: 'static + std::error::Error + std::fmt::Debug,
    LE: 'static + std::error::Error + std::fmt::Debug,
> {
    OidcRegisterClient(SdkError<RegisterClientError, Response>),
    OidcStartDeviceAuthorization(SdkError<StartDeviceAuthorizationError, Response>),
    OidcMissingVerificationUri,
    OidcCreateToken(SdkError<CreateTokenError, Response>),
    OidcTokenRefreshFailed(SdkError<CreateTokenError, Response>),
    SsoGetRoleCredentials(SdkError<GetRoleCredentialsError, Response>),
    OidcListAccounts(SdkError<ListAccountsError, Response>),
    OidcListAccountRoles(SdkError<ListAccountRolesError, Response>),
    Cache(CE),
    LockProvider(LE),
    UpstreamLocked,
}

impl<
    CE: 'static + std::error::Error + std::fmt::Debug,
    LE: 'static + std::error::Error + std::fmt::Debug,
> std::fmt::Display for Error<CE, LE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::OidcRegisterClient(err) => write!(f, "Oidc Register Client Error: {}", err),
            Error::OidcStartDeviceAuthorization(err) => {
                write!(f, "Oidc Start Device Authorization Error: {}", err)
            }
            Error::OidcMissingVerificationUri => {
                write!(
                    f,
                    "Oidc Start Device Authorization returned no verification URL"
                )
            }
            Error::OidcCreateToken(err) => write!(f, "Oidc Create Token Error: {}", err),
            Error::OidcTokenRefreshFailed(err) => {
                write!(f, "Oidc Token Refresh Failed Error: {}", err)
            }
            Error::SsoGetRoleCredentials(err) => {
                write!(f, "Sso GetRole Credentials Error: {}", err)
            }
            Error::Cache(err) => write!(f, "Cache Error: {}", err),
            Error::OidcListAccounts(err) => {
                write!(f, "Oidc List Accounts Error: {}", err)
            }
            Error::OidcListAccountRoles(err) => {
                write!(f, "Oidc List Account Roles Error: {}", err)
            }
            Error::LockProvider(err) => write!(f, "Lock Provider Error: {}", err),
            Error::UpstreamLocked => {
                write!(
                    f,
                    "Maximum retry attempts reached, upstream locked to prevent IP ban by AWS. Use aws-auth unlock to unlock."
                )
            }
        }
    }
}

impl<
    CE: 'static + std::error::Error + std::fmt::Debug,
    LE: 'static + std::error::Error + std::fmt::Debug,
> std::error::Error for Error<CE, LE>
{
}

impl<
    CE: 'static + std::error::Error + std::fmt::Debug,
    LE: 'static + std::error::Error + std::fmt::Debug,
> Error<CE, LE>
{
    /// The SSO portal API has no distinct error for a role the caller may not assume, so this is
    /// also what a forbidden role looks like -- callers must not treat it as proof of a bad token.
    fn is_unauthorized(&self) -> bool {
        match self {
            Error::SsoGetRoleCredentials(err) => matches!(
                err.as_service_error(),
                Some(GetRoleCredentialsError::UnauthorizedException(_))
            ),
            Error::OidcListAccounts(err) => matches!(
                err.as_service_error(),
                Some(ListAccountsError::UnauthorizedException(_))
            ),
            Error::OidcListAccountRoles(err) => matches!(
                err.as_service_error(),
                Some(ListAccountRolesError::UnauthorizedException(_))
            ),
            _ => false,
        }
    }
}

type Result<T, CE, LE> = std::result::Result<T, Error<CE, LE>>;

pub struct AuthManager<'a, C, L>
where
    C: 'static + ManageCache,
{
    oidc_client: OidcClient,
    sso_client: SsoClient,
    cache_manager: CacheRefMut<'a, C>,
    start_url: String,
    initial_delay: Duration,
    max_attempts: usize,
    retry_interval: Duration,
    upstream_lock: Option<L>,

    client_info: ClientInformation,
    code_writer: Box<dyn std::io::Write + 'static>,
    handle_cache: bool,
    no_browser: bool,
    access_token_reacquired: bool,
}

impl<'a, C, L> AuthManager<'a, C, L>
where
    C: 'static + ManageCache,
    C::Error: 'static + std::error::Error + std::fmt::Debug,
    L: 'static + CounterLockProvider,
{
    /// TODO: Refactor into a input type
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache_manager: impl Into<CacheRefMut<'a, C>>,
        start_url: impl Into<String>,
        sso_region: Region,
        initial_delay: Option<Duration>,
        max_attempts: Option<usize>,
        retry_interval: Option<Duration>,
        code_writer: Option<Box<dyn std::io::Write + 'static>>,
        handle_cache: bool,
        no_browser: bool,
        upstream_lock: Option<L>,
    ) -> Self {
        let sdk_config = SdkConfig::builder()
            .app_name(AppName::new(OIDC_APP_NAME).expect("Const app name should be valid"))
            .behavior_version(BehaviorVersion::latest())
            .region(sso_region.clone())
            .build();
        let oidc_client = OidcClient::new(&sdk_config);
        let sso_client = SsoClient::new(&sdk_config);

        Self {
            oidc_client,
            sso_client,
            cache_manager: cache_manager.into(),
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
            no_browser,
            upstream_lock,
            access_token_reacquired: false,
        }
    }

    async fn ensure_access_token(&mut self) -> Result<(), C::Error, L::Error> {
        if self.client_info.access_token.is_some() {
            return Ok(());
        }
        if self.client_info.refresh_token.is_some() {
            match self.refresh_access_token().await {
                Ok(()) => {
                    self.cache_manager.clear_sessions();
                    return Ok(());
                }
                // The portal session has ended. Keeping the token would dead-end every later run
                // on the same rejected refresh instead of authorizing again.
                Err(_) => self.client_info.refresh_token = None,
            }
        }
        self.create_access_token().await?;
        self.cache_manager.clear_sessions();
        Ok(())
    }

    async fn prepare_sso_and_resolve<T, F>(
        &mut self,
        resolver: F,
        ignore_cache: bool,
    ) -> Result<T, C::Error, L::Error>
    where
        F: AsyncFn(&mut Self) -> Result<T, C::Error, L::Error>,
    {
        if let Some(ref mut ul) = self.upstream_lock {
            ul.load_lock().map_err(Error::LockProvider)?;
            if ul.get_lock().is_locked() {
                return Err(Error::UpstreamLocked);
            }
        }
        if self.handle_cache {
            self.load_cache(ignore_cache);
        }
        // Re-registered before any device authorization so a client stored by an older build,
        // which was registered without a scope and so can never be issued a refresh token, is
        // replaced instead of being reused until its secret expires months later.
        let device_authorization_due =
            self.client_info.access_token.is_none() && self.client_info.refresh_token.is_none();
        if self.client_info.client_id.is_none()
            || self.client_info.client_secret.is_none()
            || device_authorization_due
        {
            self.register_client().await?;
            self.client_info.access_token = None;
            self.client_info.refresh_token = None;
        }
        let access_token_from_cache = self.client_info.access_token.is_some();
        self.ensure_access_token().await?;

        let mut result = resolver(self).await;

        // A token straight from the cache may have been revoked upstream since it was stored.
        // Bounded to one silent re-acquisition per manager because a forbidden role is
        // indistinguishable from a bad token, and batch probing relies on that case staying cheap.
        if access_token_from_cache
            && !self.access_token_reacquired
            && self.client_info.refresh_token.is_some()
            && result.as_ref().err().is_some_and(Error::is_unauthorized)
        {
            self.access_token_reacquired = true;
            self.client_info.access_token = None;
            self.client_info.access_token_expires_at = None;
            // Refreshed directly rather than through ensure_access_token, so a failure here can
            // never escalate to a device authorization in the middle of someone's command.
            result = match self.refresh_access_token().await {
                Ok(()) => {
                    self.cache_manager.clear_sessions();
                    resolver(self).await
                }
                Err(err) => {
                    self.client_info.refresh_token = None;
                    Err(err)
                }
            };
        }
        self.cache_manager.set_client_info(self.client_info.clone());
        if self.handle_cache
            && let Err(err) = self.cache_manager.commit()
        {
            if result.is_ok() {
                return Err(Error::Cache(err));
            }
            // Reported rather than returned so it cannot mask why the resolver failed.
            eprintln!("WARN: Failed to persist SSO cache: {}", err);
        }
        result
    }

    // TODO: Cache account roles
    pub async fn list_accounts(
        &mut self,
        ignore_cache: bool,
    ) -> Result<Vec<AccountInfo>, C::Error, L::Error> {
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

    // TODO: Cache account roles
    pub async fn list_account_roles(
        &mut self,
        account_id: &str,
        ignore_cache: bool,
    ) -> Result<Vec<RoleInfo>, C::Error, L::Error> {
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
    ) -> Result<Credentials, C::Error, L::Error> {
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

    pub fn load_cache(&mut self, ignore_cache: bool) {
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

    async fn register_client(&mut self) -> Result<(), C::Error, L::Error> {
        let register_client = self
            .oidc_client
            .register_client()
            .client_name(OIDC_APP_NAME)
            .client_type(OIDC_CLIENT_TYPE)
            .scopes(OIDC_SCOPE)
            .send()
            .await
            .map_err(Error::OidcRegisterClient)?;

        self.client_info.client_id = register_client.client_id;
        self.client_info.client_secret = register_client.client_secret;
        self.client_info.client_secret_expires_at =
            DateTime::from_timestamp(register_client.client_secret_expires_at, 0);

        Ok(())
    }

    async fn create_access_token(&mut self) -> Result<(), C::Error, L::Error> {
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

        let verification_uri = device_auth
            .verification_uri_complete
            .as_deref()
            .ok_or(Error::OidcMissingVerificationUri)?;

        let _ = writeln!(
            self.code_writer,
            "User Code: {}",
            device_auth.user_code.as_deref().expect(
                "Should be present. StartDeviceAuthorization fails fast in case of an error"
            )
        );

        let _ = writeln!(self.code_writer, "Verification URL: {verification_uri}");
        let browser_opened = !self.no_browser && webbrowser::open(verification_uri).is_ok();
        if !browser_opened {
            let _ = writeln!(self.code_writer, "Open the verification URL to continue.");
        }

        let device_interval = Duration::seconds(device_auth.interval as i64);
        let interval = if self.retry_interval < device_interval {
            device_interval
        } else {
            self.retry_interval
        };

        let max_attempts = if browser_opened {
            self.max_attempts
        } else {
            let remaining = Duration::seconds(device_auth.expires_in as i64) - self.initial_delay;
            let attempts = remaining.num_seconds() / interval.num_seconds().max(1);
            self.max_attempts.max(attempts.max(0) as usize)
        };

        tokio::time::sleep(self.initial_delay.to_std().unwrap_or_default()).await;

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
                Err(err) if attempts >= max_attempts => {
                    if let Some(ref mut lock) = self.upstream_lock {
                        lock.get_lock_mut().increment(1);
                        lock.save_lock().map_err(Error::LockProvider)?;
                    }
                    break Err(err);
                }
                Err(_) => {
                    tokio::time::sleep(interval.to_std().unwrap_or_default()).await;
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

    async fn refresh_access_token(&mut self) -> Result<(), C::Error, L::Error> {
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
    ) -> Result<Credentials, C::Error, L::Error> {
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
            // An unreadable expiry leaves the credentials uncacheable rather than ending the
            // command, since they are still valid for this caller right now.
            u64::try_from(credentials.expiration)
                .ok()
                .map(|millis| UNIX_EPOCH + std::time::Duration::from_millis(millis)),
            "role-credentials",
        ))
    }

    pub async fn logout(mut self) -> Result<(), C::Error, L::Error> {
        self.cache_manager.load_cache().map_err(Error::Cache)?;
        if let Some(access_token) = self.cache_manager.get_access_token() {
            let _ = self
                .sso_client
                .logout()
                .access_token(access_token)
                .send()
                .await;
        }
        self.cache_manager.cache_reset();
        self.cache_manager.commit().map_err(Error::Cache)?;
        if let Some(mut upstream_lock) = self.upstream_lock {
            upstream_lock.load_lock().map_err(Error::LockProvider)?;
            upstream_lock.get_lock_mut().reset();
            upstream_lock.save_lock().map_err(Error::LockProvider)?;
        }
        Ok(())
    }
}
