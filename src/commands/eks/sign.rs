use aws_config::Region;
use aws_sdk_ssooidc::config::Credentials;
use aws_sigv4::http_request::{
    self, SignableRequest, SignatureLocation, SigningError, SigningParams, SigningSettings,
};
use aws_sigv4::sign;
use aws_smithy_runtime_api::client::identity::Identity;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use chrono::{Duration, Local};
use http::Request;
use std::collections::HashMap;

const K8S_AWS_ID_HEADER: &str = "x-k8s-aws-id";
const TOKEN_PREFIX: &str = "k8s-aws-v1";
const DEFAULT_EXPIRTY: Duration = Duration::seconds(860);

#[derive(Debug)]
pub enum Error {
    FailedToSign(SigningError),
    InvalidRequest(http::Error),
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DEFAULT_EXEC_CREDENTIALS_KIND: &str = "ExecCredential";
pub const DEFAULT_EXEC_CREDENTIALS_API_VERSION: &str = "client.authentication.k8s.io/v1beta1";

#[derive(Debug, Deserialize, Serialize)]
pub struct K8sExecCredentialsStatus {
    #[serde(rename = "expirationTimestamp")]
    pub expiration_timestamp: DateTime<Utc>,
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct K8sExecCredentials {
    pub kind: String,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub spec: HashMap<String, serde_json::Value>,
    pub status: K8sExecCredentialsStatus,
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::FailedToSign(err) => {
                writeln!(f, "Failed to Sign Request: {}", err)
            }
            Error::InvalidRequest(err) => {
                writeln!(f, "Invalid EKS Auth request parameters: {}", err)
            }
        }
    }
}

pub fn generate_eks_credentials(
    credentials: &Credentials,
    region: &Region,
    cluster_name: &str,
    expires_in: Option<&Duration>,
) -> Result<K8sExecCredentials> {
    let expires_in = expires_in.unwrap_or(&DEFAULT_EXPIRTY);
    let credential_expiry = credentials
        .expiry()
        .map_or(Utc::now() + *expires_in, |cx_st| {
            let cx_dt: DateTime<Utc> = cx_st.into();
            if cx_dt < Utc::now() + *expires_in {
                cx_dt
            } else {
                Utc::now() + *expires_in
            }
        });

    let mut settings = SigningSettings::default();
    settings.expires_in = Some(expires_in.to_std().unwrap());
    settings.signature_location = SignatureLocation::QueryParams;

    let identity = &Identity::from(credentials.to_owned());
    let region = region.to_string();

    let params = sign::v4::SigningParams::builder()
        .identity(identity)
        .region(&region)
        .name("sts")
        .time(Local::now().into())
        .settings(settings)
        .build()
        .expect("there should not be any build errors");

    let uri =
        format!("https://sts.{region}.amazonaws.com/?Action=GetCallerIdentity&Version=2011-06-15");

    let request = SignableRequest::new(
        "GET",
        &uri,
        vec![(K8S_AWS_ID_HEADER, cluster_name)].into_iter(),
        aws_sigv4::http_request::SignableBody::Bytes(&[]),
    )
    .map_err(Error::FailedToSign)?;

    let (signing_instruction, _) = http_request::sign(request, &SigningParams::V4(params))
        .map_err(Error::FailedToSign)?
        .into_parts();

    let mut request = Request::builder()
        .uri(&uri)
        .body(())
        .map_err(Error::InvalidRequest)?;

    signing_instruction.apply_to_request_http1x(&mut request);
    let encoded_url = URL_SAFE.encode(request.uri().to_string().into_bytes());

    Ok(K8sExecCredentials {
        kind: DEFAULT_EXEC_CREDENTIALS_KIND.to_string(),
        api_version: DEFAULT_EXEC_CREDENTIALS_API_VERSION.to_string(),
        spec: HashMap::new(),
        status: K8sExecCredentialsStatus {
            expiration_timestamp: credential_expiry,
            token: format!("{}.{}", TOKEN_PREFIX, encoded_url.trim_end_matches('=')),
        },
    })
}
