use aws_config::{AppName, BehaviorVersion, Region, SdkConfig};
use aws_sdk_sso::Client as SsoClient;
use aws_sdk_sso::operation::get_role_credentials::{
    GetRoleCredentialsError, GetRoleCredentialsInput, GetRoleCredentialsOutput,
};
use aws_sdk_sso::operation::list_account_roles::{
    ListAccountRolesError, ListAccountRolesInput, ListAccountRolesOutput,
};
use aws_sdk_sso::operation::list_accounts::{
    ListAccountsError, ListAccountsInput, ListAccountsOutput,
};
use aws_sdk_sso::operation::logout::{LogoutError, LogoutInput, LogoutOutput};
use aws_sdk_ssooidc::Client as OidcClient;
use aws_sdk_ssooidc::operation::create_token::{
    CreateTokenError, CreateTokenInput, CreateTokenOutput,
};
use aws_sdk_ssooidc::operation::register_client::{
    RegisterClientError, RegisterClientInput, RegisterClientOutput,
};
use aws_sdk_ssooidc::operation::start_device_authorization::{
    StartDeviceAuthorizationError, StartDeviceAuthorizationInput, StartDeviceAuthorizationOutput,
};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_runtime_api::http::Response;
use std::cell::OnceCell;
use tokio::runtime::Runtime;

pub const APP_NAME: &str = "aws-auth";

pub type ApiResult<O, E> = Result<O, Box<SdkError<E, Response>>>;

pub trait AwsApi {
    fn register_client(
        &self,
        input: RegisterClientInput,
    ) -> ApiResult<RegisterClientOutput, RegisterClientError>;
    fn start_device_authorization(
        &self,
        input: StartDeviceAuthorizationInput,
    ) -> ApiResult<StartDeviceAuthorizationOutput, StartDeviceAuthorizationError>;
    fn create_token(
        &self,
        input: CreateTokenInput,
    ) -> ApiResult<CreateTokenOutput, CreateTokenError>;
    fn list_accounts(
        &self,
        input: ListAccountsInput,
    ) -> ApiResult<Vec<ListAccountsOutput>, ListAccountsError>;
    fn list_account_roles(
        &self,
        input: ListAccountRolesInput,
    ) -> ApiResult<Vec<ListAccountRolesOutput>, ListAccountRolesError>;
    fn get_role_credentials(
        &self,
        input: GetRoleCredentialsInput,
    ) -> ApiResult<GetRoleCredentialsOutput, GetRoleCredentialsError>;
    fn logout(&self, input: LogoutInput) -> ApiResult<LogoutOutput, LogoutError>;
}

pub struct SdkAdapter {
    runtime: Runtime,
    oidc: OidcClient,
    sso: SsoClient,
}

impl SdkAdapter {
    pub fn new(region: Region) -> Self {
        // The worker keeps hyper's pooled connections alive while the caller blocks on a request.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("Tokio runtime should build, it fails only when the OS refuses a thread or file descriptor");
        let _guard = runtime.enter();
        let sdk_config = SdkConfig::builder()
            .app_name(AppName::new(APP_NAME).expect("Const app name should be valid"))
            .behavior_version(BehaviorVersion::latest())
            .region(region)
            .build();
        let oidc = OidcClient::new(&sdk_config);
        let sso = SsoClient::new(&sdk_config);
        Self { runtime, oidc, sso }
    }
}

impl AwsApi for SdkAdapter {
    fn register_client(
        &self,
        input: RegisterClientInput,
    ) -> ApiResult<RegisterClientOutput, RegisterClientError> {
        self.runtime
            .block_on(
                self.oidc
                    .register_client()
                    .set_client_name(input.client_name)
                    .set_client_type(input.client_type)
                    .set_scopes(input.scopes)
                    .set_redirect_uris(input.redirect_uris)
                    .set_grant_types(input.grant_types)
                    .set_issuer_url(input.issuer_url)
                    .set_entitled_application_arn(input.entitled_application_arn)
                    .send(),
            )
            .map_err(Box::new)
    }

    fn start_device_authorization(
        &self,
        input: StartDeviceAuthorizationInput,
    ) -> ApiResult<StartDeviceAuthorizationOutput, StartDeviceAuthorizationError> {
        self.runtime
            .block_on(
                self.oidc
                    .start_device_authorization()
                    .set_client_id(input.client_id)
                    .set_client_secret(input.client_secret)
                    .set_start_url(input.start_url)
                    .send(),
            )
            .map_err(Box::new)
    }

    fn create_token(
        &self,
        input: CreateTokenInput,
    ) -> ApiResult<CreateTokenOutput, CreateTokenError> {
        self.runtime
            .block_on(
                self.oidc
                    .create_token()
                    .set_client_id(input.client_id)
                    .set_client_secret(input.client_secret)
                    .set_grant_type(input.grant_type)
                    .set_device_code(input.device_code)
                    .set_code(input.code)
                    .set_refresh_token(input.refresh_token)
                    .set_scope(input.scope)
                    .set_redirect_uri(input.redirect_uri)
                    .set_code_verifier(input.code_verifier)
                    .send(),
            )
            .map_err(Box::new)
    }

    fn list_accounts(
        &self,
        input: ListAccountsInput,
    ) -> ApiResult<Vec<ListAccountsOutput>, ListAccountsError> {
        self.runtime
            .block_on(
                self.sso
                    .list_accounts()
                    .set_next_token(input.next_token)
                    .set_max_results(input.max_results)
                    .set_access_token(input.access_token)
                    .into_paginator()
                    .send()
                    .collect::<Result<_, _>>(),
            )
            .map_err(Box::new)
    }

    fn list_account_roles(
        &self,
        input: ListAccountRolesInput,
    ) -> ApiResult<Vec<ListAccountRolesOutput>, ListAccountRolesError> {
        self.runtime
            .block_on(
                self.sso
                    .list_account_roles()
                    .set_next_token(input.next_token)
                    .set_max_results(input.max_results)
                    .set_access_token(input.access_token)
                    .set_account_id(input.account_id)
                    .into_paginator()
                    .send()
                    .collect::<Result<_, _>>(),
            )
            .map_err(Box::new)
    }

    fn get_role_credentials(
        &self,
        input: GetRoleCredentialsInput,
    ) -> ApiResult<GetRoleCredentialsOutput, GetRoleCredentialsError> {
        self.runtime
            .block_on(
                self.sso
                    .get_role_credentials()
                    .set_role_name(input.role_name)
                    .set_account_id(input.account_id)
                    .set_access_token(input.access_token)
                    .send(),
            )
            .map_err(Box::new)
    }

    fn logout(&self, input: LogoutInput) -> ApiResult<LogoutOutput, LogoutError> {
        self.runtime
            .block_on(
                self.sso
                    .logout()
                    .set_access_token(input.access_token)
                    .send(),
            )
            .map_err(Box::new)
    }
}

pub struct LazySdkAdapter {
    region: Region,
    inner: OnceCell<SdkAdapter>,
}

impl LazySdkAdapter {
    pub fn new(region: Region) -> Self {
        Self {
            region,
            inner: OnceCell::new(),
        }
    }

    fn inner(&self) -> &SdkAdapter {
        self.inner
            .get_or_init(|| SdkAdapter::new(self.region.clone()))
    }
}

impl AwsApi for LazySdkAdapter {
    fn register_client(
        &self,
        input: RegisterClientInput,
    ) -> ApiResult<RegisterClientOutput, RegisterClientError> {
        self.inner().register_client(input)
    }

    fn start_device_authorization(
        &self,
        input: StartDeviceAuthorizationInput,
    ) -> ApiResult<StartDeviceAuthorizationOutput, StartDeviceAuthorizationError> {
        self.inner().start_device_authorization(input)
    }

    fn create_token(
        &self,
        input: CreateTokenInput,
    ) -> ApiResult<CreateTokenOutput, CreateTokenError> {
        self.inner().create_token(input)
    }

    fn list_accounts(
        &self,
        input: ListAccountsInput,
    ) -> ApiResult<Vec<ListAccountsOutput>, ListAccountsError> {
        self.inner().list_accounts(input)
    }

    fn list_account_roles(
        &self,
        input: ListAccountRolesInput,
    ) -> ApiResult<Vec<ListAccountRolesOutput>, ListAccountRolesError> {
        self.inner().list_account_roles(input)
    }

    fn get_role_credentials(
        &self,
        input: GetRoleCredentialsInput,
    ) -> ApiResult<GetRoleCredentialsOutput, GetRoleCredentialsError> {
        self.inner().get_role_credentials(input)
    }

    fn logout(&self, input: LogoutInput) -> ApiResult<LogoutOutput, LogoutError> {
        self.inner().logout(input)
    }
}
