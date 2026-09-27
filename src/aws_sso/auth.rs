use super::api::{APP_NAME, AwsApi};
use super::cache::CacheRefMut;
use super::directory::{DirectoryCache, ListingCache};
use crate::aws_sso::cache::ManageCache;
use crate::aws_sso::types::ClientInformation;
use crate::utils::lockout::Lockout;
use aws_sdk_sso::operation::get_role_credentials::{
    GetRoleCredentialsError, GetRoleCredentialsInput,
};
use aws_sdk_sso::operation::list_account_roles::{ListAccountRolesError, ListAccountRolesInput};
use aws_sdk_sso::operation::list_accounts::{ListAccountsError, ListAccountsInput};
use aws_sdk_sso::operation::logout::LogoutInput;
use aws_sdk_sso::types::{AccountInfo, RoleInfo};
use aws_sdk_ssooidc::config::Credentials;
use aws_sdk_ssooidc::operation::create_token::{CreateTokenError, CreateTokenInput};
use aws_sdk_ssooidc::operation::register_client::{RegisterClientError, RegisterClientInput};
use aws_sdk_ssooidc::operation::start_device_authorization::{
    StartDeviceAuthorizationError, StartDeviceAuthorizationInput,
};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_runtime_api::http::Response;
use jiff::{SignedDuration, Timestamp};
use std::time::{Duration, Instant, UNIX_EPOCH};

const OIDC_CLIENT_TYPE: &str = "public";
const OIDC_SCOPE: &str = "sso:account:access";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_CREATE_TOKEN_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const CREATE_TOKEN_SLOW_DOWN_BACKOFF: Duration = Duration::from_secs(5);
const EXPECT_MESSAGE: &str = "Should be present, caller pub function assume_role asures it";

#[derive(Debug)]
pub enum Error<CE: std::error::Error> {
    OidcRegisterClient(Box<SdkError<RegisterClientError, Response>>),
    OidcStartDeviceAuthorization(Box<SdkError<StartDeviceAuthorizationError, Response>>),
    OidcMissingVerificationUri,
    OidcCreateToken(Box<SdkError<CreateTokenError, Response>>),
    OidcTokenRefreshFailed(Box<SdkError<CreateTokenError, Response>>),
    SsoGetRoleCredentials(Box<SdkError<GetRoleCredentialsError, Response>>),
    OidcListAccounts(Box<SdkError<ListAccountsError, Response>>),
    OidcListAccountRoles(Box<SdkError<ListAccountRolesError, Response>>),
    Cache(CE),
    Lockout(std::io::Error),
    UpstreamLocked,
}

impl<CE: std::error::Error> std::fmt::Display for Error<CE> {
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
            Error::Lockout(err) => write!(f, "Lockout Error: {}", err),
            Error::UpstreamLocked => {
                write!(
                    f,
                    "Maximum retry attempts reached, upstream locked to prevent IP ban by AWS. Use aws-auth unlock to unlock."
                )
            }
        }
    }
}

impl<CE: std::error::Error> std::error::Error for Error<CE> {}

impl<CE: std::error::Error> Error<CE> {
    /// The SSO portal API has no distinct error for a role the caller may not assume, so this is
    /// also what a forbidden role looks like; callers must not treat it as proof of a bad token.
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

type Result<T, CE> = std::result::Result<T, Error<CE>>;

/// The floor stops a zero from either side turning polling into a request flood.
fn poll_interval(retry_interval: Duration, device_interval: Duration) -> Duration {
    retry_interval
        .max(device_interval)
        .max(Duration::from_secs(1))
}

fn input<T, E: std::fmt::Debug>(built: std::result::Result<T, E>) -> T {
    built.expect("SDK input builders check no fields, so building cannot fail")
}

pub struct AuthManager<'a, C, A>
where
    C: ManageCache,
{
    api: A,
    cache_manager: CacheRefMut<'a, C>,
    start_url: String,
    retry_interval: Duration,
    lockout: Option<Lockout>,
    directory_cache: Option<DirectoryCache>,

    client_info: ClientInformation,
    code_writer: Box<dyn std::io::Write>,
    handle_cache: bool,
    no_browser: bool,
    access_token_reacquired: bool,
}

impl<'a, C, A> AuthManager<'a, C, A>
where
    C: ManageCache,
    A: AwsApi,
{
    /// TODO: Refactor into a input type
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api: A,
        cache_manager: impl Into<CacheRefMut<'a, C>>,
        start_url: impl Into<String>,
        retry_interval: Option<Duration>,
        code_writer: Option<Box<dyn std::io::Write>>,
        handle_cache: bool,
        no_browser: bool,
        lockout: Option<Lockout>,
    ) -> Self {
        Self {
            api,
            cache_manager: cache_manager.into(),
            start_url: start_url.into(),
            retry_interval: retry_interval.unwrap_or(DEFAULT_CREATE_TOKEN_RETRY_INTERVAL),
            client_info: ClientInformation::default(),
            code_writer: match code_writer {
                Some(cw) => cw,
                None => Box::new(std::io::stderr()),
            },
            handle_cache,
            no_browser,
            lockout,
            directory_cache: None,
            access_token_reacquired: false,
        }
    }

    pub fn with_directory_cache(mut self, directory_cache: DirectoryCache) -> Self {
        self.directory_cache = Some(directory_cache);
        self
    }

    fn ensure_access_token(&mut self) -> Result<(), C::Error> {
        if self.client_info.access_token.is_some() {
            return Ok(());
        }
        if self.client_info.refresh_token.is_some() {
            match self.refresh_access_token() {
                Ok(()) => {
                    self.cache_manager.clear_sessions();
                    return Ok(());
                }
                // The portal session has ended. Keeping the token would dead-end every later run
                // on the same rejected refresh instead of authorizing again.
                Err(_) => self.client_info.refresh_token = None,
            }
        }
        self.create_access_token()?;
        self.cache_manager.clear_sessions();
        Ok(())
    }

    fn prepare_sso_and_resolve<T, F>(
        &mut self,
        resolver: F,
        ignore_cache: bool,
    ) -> Result<T, C::Error>
    where
        F: Fn(&mut Self) -> Result<T, C::Error>,
    {
        if let Some(lockout) = &mut self.lockout
            && lockout.load().map_err(Error::Lockout)?.is_locked()
        {
            return Err(Error::UpstreamLocked);
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
            self.register_client()?;
            self.client_info.access_token = None;
            self.client_info.refresh_token = None;
        }
        let access_token_from_cache = self.client_info.access_token.is_some();
        self.ensure_access_token()?;

        let mut result = resolver(self);

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
            result = match self.refresh_access_token() {
                Ok(()) => {
                    self.cache_manager.clear_sessions();
                    resolver(self)
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

    pub fn list_accounts(
        &mut self,
        listing_cache: ListingCache,
    ) -> Result<Vec<AccountInfo>, C::Error> {
        if listing_cache == ListingCache::Use
            && let Some(accounts) = self
                .directory_cache
                .as_ref()
                .and_then(DirectoryCache::accounts)
        {
            return Ok(accounts);
        }
        let accounts: Vec<AccountInfo> = self.prepare_sso_and_resolve(
            |auth| {
                let access_token = auth
                    .client_info
                    .access_token
                    .as_deref()
                    .expect(EXPECT_MESSAGE);

                let accounts = auth
                    .api
                    .list_accounts(input(
                        ListAccountsInput::builder()
                            .access_token(access_token)
                            .build(),
                    ))
                    .map_err(Error::OidcListAccounts)?
                    .into_iter()
                    .filter_map(|res| res.account_list)
                    .flatten()
                    .collect();

                Ok(accounts)
            },
            listing_cache == ListingCache::Ignore,
        )?;
        if let Some(directory_cache) = &self.directory_cache {
            directory_cache.record_accounts(&accounts, listing_cache != ListingCache::Use);
        }
        Ok(accounts)
    }

    pub fn list_account_roles(
        &mut self,
        account_id: &str,
        listing_cache: ListingCache,
    ) -> Result<Vec<RoleInfo>, C::Error> {
        if listing_cache == ListingCache::Use
            && let Some(roles) = self
                .directory_cache
                .as_ref()
                .and_then(|directory_cache| directory_cache.roles(account_id))
        {
            return Ok(roles);
        }
        let roles: Vec<RoleInfo> = self.prepare_sso_and_resolve(
            |auth| {
                let access_token = auth
                    .client_info
                    .access_token
                    .as_deref()
                    .expect(EXPECT_MESSAGE);
                let roles = auth
                    .api
                    .list_account_roles(input(
                        ListAccountRolesInput::builder()
                            .account_id(account_id)
                            .access_token(access_token)
                            .build(),
                    ))
                    .map_err(Error::OidcListAccountRoles)?
                    .into_iter()
                    .filter_map(|res| res.role_list)
                    .flatten()
                    .collect();
                Ok(roles)
            },
            listing_cache == ListingCache::Ignore,
        )?;
        if let Some(directory_cache) = &self.directory_cache {
            directory_cache.record_roles(account_id, &roles, listing_cache != ListingCache::Use);
        }
        Ok(roles)
    }

    pub fn assume_role(
        &mut self,
        account_id: &str,
        role_name: &str,
        refresh_sts_token: bool,
        ignore_cache: bool,
    ) -> Result<Credentials, C::Error> {
        self.prepare_sso_and_resolve(
            |auth| {
                let credentials = if refresh_sts_token {
                    auth.resolve_credentials(role_name, account_id)?
                } else if let Some(cached_credentials) =
                    auth.cache_manager.get_session(account_id, role_name)
                {
                    Credentials::from(cached_credentials.clone())
                } else {
                    auth.resolve_credentials(role_name, account_id)?
                };
                auth.cache_manager
                    .set_session(account_id, role_name, credentials.clone());
                Ok(credentials)
            },
            ignore_cache,
        )
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

    fn register_client(&mut self) -> Result<(), C::Error> {
        let register_client = self
            .api
            .register_client(input(
                RegisterClientInput::builder()
                    .client_name(APP_NAME)
                    .client_type(OIDC_CLIENT_TYPE)
                    .scopes(OIDC_SCOPE)
                    .build(),
            ))
            .map_err(Error::OidcRegisterClient)?;

        self.client_info.client_id = register_client.client_id;
        self.client_info.client_secret = register_client.client_secret;
        self.client_info.client_secret_expires_at =
            Timestamp::from_second(register_client.client_secret_expires_at).ok();

        Ok(())
    }

    fn create_access_token(&mut self) -> Result<(), C::Error> {
        let device_auth = self
            .api
            .start_device_authorization(input(
                StartDeviceAuthorizationInput::builder()
                    .client_id(self.client_info.client_id.as_deref().expect(EXPECT_MESSAGE))
                    .client_secret(
                        self.client_info
                            .client_secret
                            .as_deref()
                            .expect(EXPECT_MESSAGE),
                    )
                    .start_url(&self.start_url)
                    .build(),
            ))
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

        let device_interval = Duration::from_secs(device_auth.interval.max(0) as u64);
        let mut interval = poll_interval(self.retry_interval, device_interval);
        let deadline = Instant::now() + Duration::from_secs(device_auth.expires_in.max(0) as u64);

        std::thread::sleep(interval);

        let create_token = loop {
            match self.api.create_token(input(
                CreateTokenInput::builder()
                    .client_id(self.client_info.client_id.as_deref().expect(EXPECT_MESSAGE))
                    .client_secret(
                        self.client_info
                            .client_secret
                            .as_deref()
                            .expect(EXPECT_MESSAGE),
                    )
                    .grant_type(GRANT_TYPE)
                    .device_code(device_auth.device_code.as_deref().expect(EXPECT_MESSAGE))
                    .build(),
            )) {
                Ok(token) => break Ok(token),
                Err(err) => {
                    let reached_endpoint = match err.as_service_error() {
                        Some(CreateTokenError::AuthorizationPendingException(_)) => true,
                        Some(CreateTokenError::SlowDownException(_)) => {
                            interval += CREATE_TOKEN_SLOW_DOWN_BACKOFF;
                            true
                        }
                        // A request that never landed carries no verdict on the device code, and
                        // the window here is long enough that a dropped network is expected, so
                        // it is polled through rather than ending someone's login.
                        None if matches!(
                            err.as_ref(),
                            SdkError::DispatchFailure(_) | SdkError::TimeoutError(_)
                        ) =>
                        {
                            false
                        }
                        _ => break Err(err),
                    };
                    // Stop rather than sleep into a code that expires before the next poll.
                    if Instant::now() + interval >= deadline {
                        // Only a round that actually reached AWS is evidence of the repeated
                        // authorizations the lock exists to slow down.
                        if reached_endpoint && let Some(lockout) = &mut self.lockout {
                            let mut loaded = lockout.load().map_err(Error::Lockout)?;
                            loaded.increment();
                            loaded.save().map_err(Error::Lockout)?;
                        }
                        break Err(err);
                    }
                    std::thread::sleep(interval);
                }
            }
        }
        .map_err(Error::OidcCreateToken)?;

        self.client_info.access_token = create_token.access_token;
        self.client_info.refresh_token = create_token.refresh_token;
        self.client_info.access_token_expires_at =
            Some(Timestamp::now() + SignedDuration::from_secs(create_token.expires_in.into()));
        // A new device authorization may have signed in as another identity.
        self.clear_directory_cache();

        if let Some(lockout) = &mut self.lockout {
            let mut loaded = lockout.load().map_err(Error::Lockout)?;
            loaded.reset();
            loaded.save().map_err(Error::Lockout)?;
        }
        Ok(())
    }

    fn refresh_access_token(&mut self) -> Result<(), C::Error> {
        let create_token = self
            .api
            .create_token(input(
                CreateTokenInput::builder()
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
                    .build(),
            ))
            .map_err(Error::OidcTokenRefreshFailed)?;
        self.client_info.access_token = create_token.access_token;
        self.client_info.refresh_token = create_token.refresh_token;
        self.client_info.access_token_expires_at =
            Some(Timestamp::now() + SignedDuration::from_secs(create_token.expires_in.into()));
        Ok(())
    }

    fn resolve_credentials(
        &self,
        role_name: &str,
        account_id: &str,
    ) -> Result<Credentials, C::Error> {
        let credentials = self
            .api
            .get_role_credentials(input(
                GetRoleCredentialsInput::builder()
                    .role_name(role_name)
                    .account_id(account_id)
                    .access_token(
                        self.client_info
                            .access_token
                            .as_deref()
                            .expect(EXPECT_MESSAGE),
                    )
                    .build(),
            ))
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

    fn clear_directory_cache(&self) {
        if let Some(directory_cache) = &self.directory_cache
            && let Err(err) = directory_cache.clear()
        {
            eprintln!("WARN: Failed to clear SSO directory cache: {err}");
        }
    }

    pub fn logout(mut self) -> Result<(), C::Error> {
        // A missing or corrupt cache has no token to revoke, and is reset below all the same.
        if self.cache_manager.load_cache().is_ok()
            && let Some(access_token) = self.cache_manager.get_access_token()
        {
            let _ = self.api.logout(input(
                LogoutInput::builder().access_token(access_token).build(),
            ));
        }
        self.clear_directory_cache();
        self.cache_manager.cache_reset();
        self.cache_manager.commit().map_err(Error::Cache)?;
        if let Some(lockout) = &mut self.lockout {
            let mut loaded = lockout.load().map_err(Error::Lockout)?;
            loaded.reset();
            loaded.save().map_err(Error::Lockout)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
