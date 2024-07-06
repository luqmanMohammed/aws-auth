use std::time::{Duration, SystemTime};

use aws_config::BehaviorVersion;
use aws_sdk_sts::config::ProvideCredentials;
use aws_sigv4::http_request::SignableRequest;
use base64::{engine::general_purpose::URL_SAFE, Engine};

pub async fn generate_eks_auth_token() -> Result<(), Box<dyn std::error::Error>> {
    let sdk_config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    let creds = sdk_config
        .credentials_provider()
        .ok_or("")?
        .provide_credentials()
        .await?;

    let mut settings = aws_sigv4::http_request::SigningSettings::default();
    settings.expires_in = Some(Duration::from_secs(840));
    settings.signature_location = aws_sigv4::http_request::SignatureLocation::QueryParams;

    let identity = &aws_smithy_runtime_api::client::identity::Identity::from(creds);

    let params = aws_sigv4::sign::v4::SigningParams::builder()
        .identity(identity)
        .region("eu-west-2")
        .name("sts")
        .time(SystemTime::now())
        .settings(settings)
        .build()?;

    let uri = String::from(
        "https://sts.eu-west-2.amazonaws.com/?Action=GetCallerIdentity&Version=2011-06-15",
    );
    let request = SignableRequest::new(
        "GET",
        &uri,
        vec![("x-k8s-aws-id", "shared-services")].into_iter(),
        aws_sigv4::http_request::SignableBody::Bytes(&[]),
    )?;

    let (signing_instruction, _) = aws_sigv4::http_request::sign(
        request,
        &aws_sigv4::http_request::SigningParams::V4(params),
    )?
    .into_parts();

    let mut req = http::request::Request::builder().uri(&uri).body(())?;
    signing_instruction.apply_to_request_http1x(&mut req);

    let encoded_url = URL_SAFE.encode(req.uri().to_string().into_bytes());
    println!("k8s-aws-v1.{}", encoded_url.trim_end_matches('='));

    Ok(())
}
