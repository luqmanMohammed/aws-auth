use crate::eks::generate_eks_auth_token;
use std::{thread, time::Duration};

use aws_config::{AppName, BehaviorVersion, Region};

const MAX_RETRIES: u8 = 20;
const INTERVAL: Duration = Duration::from_secs(5);

pub async fn sso() -> Result<(), Box<dyn std::error::Error>> {
    let sdkconfig = aws_config::SdkConfig::builder()
        .app_name(AppName::new("aws-sso-eks-auth")?)
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("eu-west-2"))
        .build();

    let ssooidc_client = aws_sdk_ssooidc::Client::new(&sdkconfig);
    let register_client = ssooidc_client
        .register_client()
        .client_name("aws-sso-eks-auth")
        .client_type("public")
        .send()
        .await?;

    let client_id = register_client.client_id.ok_or("No client ID")?;
    let client_secret = register_client.client_secret.ok_or("No client secret")?;

    let device_auth = ssooidc_client
        .start_device_authorization()
        .client_id(&client_id)
        .client_secret(&client_secret)
        .start_url("https://faker.awsapps.com/start")
        .send()
        .await?;

    let veri_uri = device_auth
        .verification_uri_complete()
        .ok_or("verification uri error")?;
    webbrowser::open(veri_uri)?;

    let device_code = device_auth.device_code.ok_or("Device Code Not Found")?;
    eprintln!("Device Code {:?}", device_auth.user_code);
    let mut tries = 0;
    let token = loop {
        if let Ok(token) = ssooidc_client
            .create_token()
            .client_id(&client_id)
            .client_secret(&client_secret)
            .grant_type("urn:ietf:params:oauth:grant-type:device_code")
            .device_code(&device_code)
            .send()
            .await
        {
            break Some(token);
        }
        if tries > MAX_RETRIES {
            break None;
        }
        thread::sleep(INTERVAL);
        tries += 1;
    }
    .ok_or("Token Error")?;
    println!("{:?}", token);

    let sso_client = aws_sdk_sso::Client::new(&sdkconfig);
    let credentials = sso_client
        .get_role_credentials()
        .role_name("<faker>")
        .account_id("<faker>")
        .access_token(
            token
                .access_token
                .as_ref()
                .ok_or("Access Token not found")?,
        )
        .send()
        .await?
        .role_credentials
        .ok_or("Role Creds not found")?;

    let credentials = aws_sdk_sso::config::Credentials::new(
        credentials
            .access_key_id
            .as_ref()
            .ok_or("Access Key Not Found")?,
        credentials
            .secret_access_key
            .as_ref()
            .ok_or("Secret Access Key Not Found")?,
        credentials.session_token,
        None,
        "awsso-oidc",
    );

    generate_eks_auth_token(&credentials).await?;

    Ok(())
}
