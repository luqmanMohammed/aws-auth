use std::time::SystemTime;

use aws_sdk_ssooidc::config::Credentials;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CredentialsWrapper {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub expires_after: Option<SystemTime>,
}

impl From<Credentials> for CredentialsWrapper {
    fn from(value: Credentials) -> Self {
        Self {
            access_key_id: value.access_key_id().to_string(),
            secret_access_key: value.secret_access_key().to_string(),
            session_token: value.session_token().map(ToString::to_string),
            expires_after: value.expiry(),
        }
    }
}

impl From<CredentialsWrapper> for Credentials {
    fn from(value: CredentialsWrapper) -> Credentials {
        aws_sdk_ssooidc::config::Credentials::new(
            value.access_key_id,
            value.secret_access_key,
            value.session_token,
            value.expires_after,
            "credential-wrapper",
        )
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ClientInformation {
    pub start_url: Option<String>,
    pub client_secret_expires_at: Option<DateTime<Utc>>,
    pub access_token_expires_at: Option<DateTime<Utc>>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
}
