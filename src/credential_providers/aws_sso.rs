use crate::types::{K8sExecCredentials, K8sExecCredentialsStatus};
use aws_config::Region;
use aws_sdk_ssooidc::config::Credentials;
use aws_sigv4::http_request::{
    self, SignableRequest, SignatureLocation, SigningError, SigningParams, SigningSettings,
};
use aws_sigv4::sign;
use aws_smithy_runtime_api::client::identity::Identity;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use http::request::Request;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

const K8S_AWS_ID_HEADER: &str = "x-k8s-aws-id";
const TOKEN_PREFIX: &str = "k8s-aws-v1";
const DEFAULT_EXPIRTY: Duration = Duration::from_secs(860);

#[derive(Debug)]
pub enum Error {
    SigningError(SigningError),
    RequestBuildError(http::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SigningError(err) => writeln!(f, "Signing Error: {}", err),
            Error::RequestBuildError(err) => writeln!(f, "Request build Error: {}", err),
        }
    }
}

impl std::error::Error for Error {}

pub async fn generate_auth_credentials(
    credentials: &Credentials,
    region: &Region,
    cluster_name: &str,
    expires_in: Option<Duration>,
) -> Result<K8sExecCredentials, Error> {
    let expires_in = expires_in.unwrap_or(DEFAULT_EXPIRTY);

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
        .expect("Assert: No build errors");

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
    .map_err(Error::SigningError)?;

    let (signing_instruction, _) = http_request::sign(request, &SigningParams::V4(params))
        .map_err(Error::SigningError)?
        .into_parts();

    let mut request = Request::builder()
        .uri(&uri)
        .body(())
        .map_err(Error::RequestBuildError)?;

    signing_instruction.apply_to_request_http1x(&mut request);
    let encoded_url = URL_SAFE.encode(request.uri().to_string().into_bytes());

    Ok(K8sExecCredentials {
        kind: String::from(""),
        api_version: String::from(""),
        spec: HashMap::new(),
        status: K8sExecCredentialsStatus {
            expiration_timestamp: chrono::Utc::now()
                + chrono::Duration::seconds(expires_in.as_secs().try_into().unwrap()),
            token: format!("{}.{}", TOKEN_PREFIX, encoded_url.trim_end_matches('=')),
        },
    })
}
