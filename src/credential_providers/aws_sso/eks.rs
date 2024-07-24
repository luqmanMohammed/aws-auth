use crate::types::{
    K8sExecCredentials, K8sExecCredentialsStatus, DEFAULT_EXEC_CREDENTIALS_API_VERSION,
    DEFAULT_EXEC_CREDENTIALS_KIND,
};
use aws_config::Region;
use aws_sdk_ssooidc::config::Credentials;
use aws_sigv4::http_request::{
    self, SignableRequest, SignatureLocation, SigningError, SigningParams, SigningSettings,
};
use aws_sigv4::sign;
use aws_smithy_runtime_api::client::identity::Identity;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use chrono::{Duration, Local};
use http::{Error, Request};
use std::collections::HashMap;

const K8S_AWS_ID_HEADER: &str = "x-k8s-aws-id";
const TOKEN_PREFIX: &str = "k8s-aws-v1";
const DEFAULT_EXPIRTY: Duration = Duration::seconds(860);

#[derive(Debug)]
pub enum GenerateEksCredentialsError {
    FailedToSign(SigningError),
    InvalidRequest(Error),
}

impl std::error::Error for GenerateEksCredentialsError {}

impl std::fmt::Display for GenerateEksCredentialsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerateEksCredentialsError::FailedToSign(err) => {
                writeln!(f, "Failed to Sign Request: {}", err)
            }
            GenerateEksCredentialsError::InvalidRequest(err) => {
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
) -> Result<K8sExecCredentials, GenerateEksCredentialsError> {
    let expires_in = expires_in.unwrap_or(&DEFAULT_EXPIRTY);

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
    .map_err(GenerateEksCredentialsError::FailedToSign)?;

    let (signing_instruction, _) = http_request::sign(request, &SigningParams::V4(params))
        .map_err(GenerateEksCredentialsError::FailedToSign)?
        .into_parts();

    let mut request = Request::builder()
        .uri(&uri)
        .body(())
        .map_err(GenerateEksCredentialsError::InvalidRequest)?;

    signing_instruction.apply_to_request_http1x(&mut request);
    let encoded_url = URL_SAFE.encode(request.uri().to_string().into_bytes());

    Ok(K8sExecCredentials {
        kind: DEFAULT_EXEC_CREDENTIALS_KIND.to_string(),
        api_version: DEFAULT_EXEC_CREDENTIALS_API_VERSION.to_string(),
        spec: HashMap::new(),
        status: K8sExecCredentialsStatus {
            expiration_timestamp: chrono::Utc::now() + *expires_in,
            token: format!("{}.{}", TOKEN_PREFIX, encoded_url.trim_end_matches('=')),
        },
    })
}
