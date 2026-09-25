// Tests were written by AI (Claude Opus 5), not reviewed by Author

use super::*;
use crate::aws_sso::api::ApiResult;
use crate::aws_sso::cache::Cache;
use crate::utils::lock::DecayingJsonCounterLockProvider;
use crate::utils::test_support::TempDir;
use aws_sdk_sso::operation::get_role_credentials::GetRoleCredentialsOutput;
use aws_sdk_sso::operation::list_account_roles::ListAccountRolesOutput;
use aws_sdk_sso::operation::list_accounts::ListAccountsOutput;
use aws_sdk_sso::operation::logout::{LogoutError, LogoutOutput};
use aws_sdk_sso::types::RoleCredentials;
use aws_sdk_ssooidc::operation::create_token::CreateTokenOutput;
use aws_sdk_ssooidc::operation::register_client::RegisterClientOutput;
use aws_sdk_ssooidc::operation::start_device_authorization::StartDeviceAuthorizationOutput;
use aws_sdk_ssooidc::types::error::{
    AccessDeniedException, AuthorizationPendingException, InvalidGrantException, SlowDownException,
};
use aws_smithy_runtime_api::client::result::ConnectorError;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::SystemTime;

const START_URL: &str = "https://example.awsapps.com/start";

type Script<O, E> = RefCell<VecDeque<ApiResult<O, E>>>;

#[derive(Default)]
struct FakeAws {
    register_client: Script<RegisterClientOutput, RegisterClientError>,
    start_device_authorization:
        Script<StartDeviceAuthorizationOutput, StartDeviceAuthorizationError>,
    create_token: Script<CreateTokenOutput, CreateTokenError>,
    get_role_credentials: Script<GetRoleCredentialsOutput, GetRoleCredentialsError>,
    calls: RefCell<Vec<&'static str>>,
    create_token_inputs: RefCell<Vec<CreateTokenInput>>,
    role_access_tokens: RefCell<Vec<String>>,
}

impl FakeAws {
    fn next<O, E>(&self, name: &'static str, script: &Script<O, E>) -> ApiResult<O, E> {
        self.calls.borrow_mut().push(name);
        script
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| panic!("unscripted {name} call"))
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl AwsApi for FakeAws {
    fn register_client(
        &self,
        _: RegisterClientInput,
    ) -> ApiResult<RegisterClientOutput, RegisterClientError> {
        self.next("register_client", &self.register_client)
    }

    fn start_device_authorization(
        &self,
        _: StartDeviceAuthorizationInput,
    ) -> ApiResult<StartDeviceAuthorizationOutput, StartDeviceAuthorizationError> {
        self.next(
            "start_device_authorization",
            &self.start_device_authorization,
        )
    }

    fn create_token(
        &self,
        input: CreateTokenInput,
    ) -> ApiResult<CreateTokenOutput, CreateTokenError> {
        self.create_token_inputs.borrow_mut().push(input);
        self.next("create_token", &self.create_token)
    }

    fn list_accounts(
        &self,
        _: ListAccountsInput,
    ) -> ApiResult<Vec<ListAccountsOutput>, ListAccountsError> {
        unimplemented!("not exercised")
    }

    fn list_account_roles(
        &self,
        _: ListAccountRolesInput,
    ) -> ApiResult<Vec<ListAccountRolesOutput>, ListAccountRolesError> {
        unimplemented!("not exercised")
    }

    fn get_role_credentials(
        &self,
        input: GetRoleCredentialsInput,
    ) -> ApiResult<GetRoleCredentialsOutput, GetRoleCredentialsError> {
        self.role_access_tokens
            .borrow_mut()
            .push(input.access_token.unwrap_or_default());
        self.next("get_role_credentials", &self.get_role_credentials)
    }

    fn logout(&self, _: LogoutInput) -> ApiResult<LogoutOutput, LogoutError> {
        unimplemented!("not exercised")
    }
}

#[derive(Default)]
struct MemCache {
    cache: Cache,
}

impl ManageCache for MemCache {
    type Error = std::io::Error;
    fn load_cache(&mut self) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
    fn commit(&self) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
    fn get_cache_as_ref(&self) -> &Cache {
        &self.cache
    }
    fn get_cache_as_mut(&mut self) -> &mut Cache {
        &mut self.cache
    }
}

type Manager = AuthManager<'static, MemCache, DecayingJsonCounterLockProvider, FakeAws>;

fn manager(
    cache: MemCache,
    api: FakeAws,
    lock: Option<DecayingJsonCounterLockProvider>,
) -> Manager {
    AuthManager::new(
        api,
        cache,
        START_URL,
        Some(Duration::ZERO),
        Some(Box::new(std::io::sink())),
        true,
        true,
        lock,
    )
}

fn cached(access_token: Option<&str>, refresh_token: Option<&str>) -> MemCache {
    let mut cache = MemCache::default();
    cache.set_client_info(ClientInformation {
        start_url: Some(START_URL.to_string()),
        client_id: Some("client-id".to_string()),
        client_secret: Some("client-secret".to_string()),
        client_secret_expires_at: Some(Utc::now() + TimeDelta::days(30)),
        access_token: access_token.map(str::to_string),
        access_token_expires_at: access_token.map(|_| Utc::now() + TimeDelta::hours(1)),
        refresh_token: refresh_token.map(str::to_string),
    });
    cache
}

fn lock(dir: &TempDir, threshold: u64) -> DecayingJsonCounterLockProvider {
    DecayingJsonCounterLockProvider::new(dir.path(), "lock", threshold, None)
}

fn service_error<E>(err: E) -> Box<SdkError<E, Response>> {
    Box::new(SdkError::service_error(
        err,
        Response::new(400.try_into().expect("valid status"), "".into()),
    ))
}

fn dispatch_failure<E>() -> Box<SdkError<E, Response>> {
    Box::new(SdkError::dispatch_failure(ConnectorError::io(
        "connection reset".into(),
    )))
}

fn registered() -> ApiResult<RegisterClientOutput, RegisterClientError> {
    Ok(RegisterClientOutput::builder()
        .client_id("client-id")
        .client_secret("client-secret")
        .client_secret_expires_at((Utc::now() + TimeDelta::days(30)).timestamp())
        .build())
}

fn device_authorization(
    expires_in: i32,
) -> ApiResult<StartDeviceAuthorizationOutput, StartDeviceAuthorizationError> {
    Ok(StartDeviceAuthorizationOutput::builder()
        .device_code("device-code")
        .user_code("user-code")
        .verification_uri_complete("https://device.example")
        .expires_in(expires_in)
        .interval(0)
        .build())
}

fn token(access_token: &str) -> ApiResult<CreateTokenOutput, CreateTokenError> {
    Ok(CreateTokenOutput::builder()
        .access_token(access_token)
        .refresh_token(format!("{access_token}-refresh"))
        .expires_in(3600)
        .build())
}

fn pending() -> ApiResult<CreateTokenOutput, CreateTokenError> {
    Err(service_error(
        CreateTokenError::AuthorizationPendingException(
            AuthorizationPendingException::builder().build(),
        ),
    ))
}

fn role_credentials() -> ApiResult<GetRoleCredentialsOutput, GetRoleCredentialsError> {
    let expiration = (Utc::now() + TimeDelta::hours(1)).timestamp_millis();
    Ok(GetRoleCredentialsOutput::builder()
        .role_credentials(
            RoleCredentials::builder()
                .access_key_id("AKIA")
                .secret_access_key("secret")
                .session_token("session")
                .expiration(expiration)
                .build(),
        )
        .build())
}

fn unauthorized() -> ApiResult<GetRoleCredentialsOutput, GetRoleCredentialsError> {
    Err(service_error(
        GetRoleCredentialsError::UnauthorizedException(
            aws_sdk_sso::types::error::UnauthorizedException::builder().build(),
        ),
    ))
}

fn assume(manager: &mut Manager) -> Result<Credentials, std::io::Error, std::io::Error> {
    manager.assume_role("111111111111", "Admin", false, false)
}

#[test]
fn a_first_login_registers_authorizes_and_polls_through_pending() {
    let api = FakeAws::default();
    api.register_client.borrow_mut().push_back(registered());
    api.start_device_authorization
        .borrow_mut()
        .push_back(device_authorization(600));
    api.create_token
        .borrow_mut()
        .extend([pending(), token("fresh")]);
    api.get_role_credentials
        .borrow_mut()
        .push_back(role_credentials());
    let mut manager = manager(MemCache::default(), api, None);

    let credentials = assume(&mut manager).expect("login should succeed");

    assert_eq!(credentials.access_key_id(), "AKIA");
    assert_eq!(
        manager.api.calls(),
        [
            "register_client",
            "start_device_authorization",
            "create_token",
            "create_token",
            "get_role_credentials"
        ]
    );
    assert_eq!(*manager.api.role_access_tokens.borrow(), ["fresh"]);
    assert_eq!(manager.cache_manager.get_access_token(), Some("fresh"));
    assert_eq!(
        manager.cache_manager.get_refresh_token(),
        Some("fresh-refresh")
    );
}

#[test]
fn a_cached_access_token_is_used_without_touching_oidc() {
    let api = FakeAws::default();
    api.get_role_credentials
        .borrow_mut()
        .push_back(role_credentials());
    let mut manager = manager(cached(Some("cached"), Some("cached-refresh")), api, None);

    assume(&mut manager).expect("cached token should be enough");

    assert_eq!(manager.api.calls(), ["get_role_credentials"]);
    assert_eq!(*manager.api.role_access_tokens.borrow(), ["cached"]);
}

#[test]
fn a_cached_session_is_returned_without_calling_aws() {
    let mut cache = cached(Some("cached"), None);
    cache.set_session(
        "111111111111",
        "Admin",
        Credentials::new(
            "AKIA-CACHED",
            "secret",
            None,
            Some(SystemTime::now() + Duration::from_secs(3600)),
            "test",
        ),
    );
    let mut manager = manager(cache, FakeAws::default(), None);

    let credentials = assume(&mut manager).expect("cached session should be returned");

    assert_eq!(credentials.access_key_id(), "AKIA-CACHED");
    assert!(manager.api.calls().is_empty());
}

#[test]
fn an_expired_access_token_is_refreshed_instead_of_reauthorized() {
    let api = FakeAws::default();
    api.create_token.borrow_mut().push_back(token("refreshed"));
    api.get_role_credentials
        .borrow_mut()
        .push_back(role_credentials());
    let mut manager = manager(cached(None, Some("cached-refresh")), api, None);

    assume(&mut manager).expect("refresh should succeed");

    assert_eq!(
        manager.api.calls(),
        ["create_token", "get_role_credentials"]
    );
    let inputs = manager.api.create_token_inputs.borrow();
    assert_eq!(inputs[0].grant_type(), Some("refresh_token"));
    assert_eq!(inputs[0].refresh_token(), Some("cached-refresh"));
    assert_eq!(*manager.api.role_access_tokens.borrow(), ["refreshed"]);
}

#[test]
fn a_rejected_refresh_falls_back_to_device_authorization() {
    let api = FakeAws::default();
    api.create_token.borrow_mut().extend([
        Err(service_error(CreateTokenError::InvalidGrantException(
            InvalidGrantException::builder().build(),
        ))),
        token("fresh"),
    ]);
    api.start_device_authorization
        .borrow_mut()
        .push_back(device_authorization(600));
    api.get_role_credentials
        .borrow_mut()
        .push_back(role_credentials());
    let mut manager = manager(cached(None, Some("dead-refresh")), api, None);

    assume(&mut manager).expect("device authorization should recover");

    assert_eq!(
        manager.api.calls(),
        [
            "create_token",
            "start_device_authorization",
            "create_token",
            "get_role_credentials"
        ]
    );
    assert_eq!(
        manager.api.create_token_inputs.borrow()[1].grant_type(),
        Some(GRANT_TYPE)
    );
}

#[test]
fn a_revoked_cached_token_is_refreshed_and_retried_once_per_manager() {
    let api = FakeAws::default();
    api.get_role_credentials.borrow_mut().extend([
        unauthorized(),
        role_credentials(),
        unauthorized(),
    ]);
    api.create_token.borrow_mut().push_back(token("refreshed"));
    let mut manager = manager(cached(Some("revoked"), Some("cached-refresh")), api, None);

    assume(&mut manager).expect("the retry with a refreshed token should succeed");
    assert_eq!(
        *manager.api.role_access_tokens.borrow(),
        ["revoked", "refreshed"]
    );

    let err = manager
        .assume_role("222222222222", "Admin", false, false)
        .expect_err("a forbidden role must not trigger another refresh");
    assert!(matches!(err, Error::SsoGetRoleCredentials(_)));
    assert_eq!(
        manager.api.calls(),
        [
            "get_role_credentials",
            "create_token",
            "get_role_credentials",
            "get_role_credentials"
        ]
    );
}

#[test]
fn a_dropped_network_while_polling_does_not_end_the_login() {
    let api = FakeAws::default();
    api.register_client.borrow_mut().push_back(registered());
    api.start_device_authorization
        .borrow_mut()
        .push_back(device_authorization(600));
    api.create_token
        .borrow_mut()
        .extend([Err(dispatch_failure()), token("fresh")]);
    api.get_role_credentials
        .borrow_mut()
        .push_back(role_credentials());
    let mut manager = manager(MemCache::default(), api, None);

    assume(&mut manager).expect("polling should continue through a dispatch failure");
}

#[test]
fn an_unexpected_create_token_error_ends_the_login() {
    let api = FakeAws::default();
    api.register_client.borrow_mut().push_back(registered());
    api.start_device_authorization
        .borrow_mut()
        .push_back(device_authorization(600));
    api.create_token.borrow_mut().push_back(Err(service_error(
        CreateTokenError::AccessDeniedException(AccessDeniedException::builder().build()),
    )));
    let mut manager = manager(MemCache::default(), api, None);

    let err = assume(&mut manager).expect_err("access denied should end the login");

    assert!(matches!(err, Error::OidcCreateToken(_)));
    assert_eq!(
        manager.api.calls(),
        [
            "register_client",
            "start_device_authorization",
            "create_token"
        ]
    );
}

#[test]
fn a_slow_down_past_the_code_expiry_ends_the_login_and_counts_against_the_lock() {
    let dir = TempDir::new("auth-slow-down");
    let api = FakeAws::default();
    api.register_client.borrow_mut().push_back(registered());
    api.start_device_authorization
        .borrow_mut()
        .push_back(device_authorization(1));
    api.create_token.borrow_mut().push_back(Err(service_error(
        CreateTokenError::SlowDownException(SlowDownException::builder().build()),
    )));
    let mut manager = manager(MemCache::default(), api, Some(lock(&dir, 1)));

    let err = assume(&mut manager).expect_err("the backoff outlasts the code");

    assert!(matches!(err, Error::OidcCreateToken(_)));
    assert!(
        manager
            .upstream_lock
            .as_ref()
            .unwrap()
            .get_lock()
            .is_locked()
    );
}

#[test]
fn a_final_poll_that_never_reached_aws_does_not_count_against_the_lock() {
    let dir = TempDir::new("auth-dispatch-lock");
    let api = FakeAws::default();
    api.register_client.borrow_mut().push_back(registered());
    api.start_device_authorization
        .borrow_mut()
        .push_back(device_authorization(0));
    api.create_token
        .borrow_mut()
        .push_back(Err(dispatch_failure()));
    let mut manager = manager(MemCache::default(), api, Some(lock(&dir, 1)));

    assume(&mut manager).expect_err("the code has expired");

    assert!(
        manager
            .upstream_lock
            .as_ref()
            .unwrap()
            .get_lock()
            .is_clear()
    );
}

#[test]
fn a_locked_upstream_fails_before_calling_aws() {
    let dir = TempDir::new("auth-locked");
    let mut held = lock(&dir, 1);
    held.load_lock().unwrap();
    held.get_lock_mut().increment(1);
    held.save_lock().unwrap();
    let mut manager = manager(MemCache::default(), FakeAws::default(), Some(lock(&dir, 1)));

    let err = assume(&mut manager).expect_err("the lock should hold");

    assert!(matches!(err, Error::UpstreamLocked));
    assert!(manager.api.calls().is_empty());
}

#[test]
fn a_completed_login_clears_the_lock_count() {
    let dir = TempDir::new("auth-lock-reset");
    let mut held = lock(&dir, 3);
    held.load_lock().unwrap();
    held.get_lock_mut().increment(1);
    held.save_lock().unwrap();
    let api = FakeAws::default();
    api.register_client.borrow_mut().push_back(registered());
    api.start_device_authorization
        .borrow_mut()
        .push_back(device_authorization(600));
    api.create_token.borrow_mut().push_back(token("fresh"));
    api.get_role_credentials
        .borrow_mut()
        .push_back(role_credentials());
    let mut manager = manager(MemCache::default(), api, Some(lock(&dir, 3)));

    assume(&mut manager).expect("login should succeed");

    assert!(
        manager
            .upstream_lock
            .as_ref()
            .unwrap()
            .get_lock()
            .is_clear()
    );
}

#[test]
fn aws_wins_when_it_asks_for_slower_polling_than_configured() {
    assert_eq!(
        poll_interval(Duration::from_secs(5), Duration::from_secs(30)),
        Duration::from_secs(30)
    );
}

#[test]
fn the_configured_interval_wins_when_it_is_the_slower_of_the_two() {
    assert_eq!(
        poll_interval(Duration::from_secs(30), Duration::from_secs(5)),
        Duration::from_secs(30)
    );
}

#[test]
fn a_zero_from_either_side_never_yields_a_sleepless_poll() {
    assert_eq!(
        poll_interval(Duration::ZERO, Duration::ZERO),
        Duration::from_secs(1)
    );
}

#[test]
fn the_default_interval_clears_the_floor_untouched() {
    assert_eq!(
        poll_interval(DEFAULT_CREATE_TOKEN_RETRY_INTERVAL, Duration::ZERO),
        DEFAULT_CREATE_TOKEN_RETRY_INTERVAL
    );
}
