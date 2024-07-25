use crate::credential_providers::aws_sso::types::{ClientInformation, CredentialsWrapper};
use aws_sdk_ssooidc::config::Credentials;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const EXPIRATION_BUFFER: Duration = Duration::minutes(5);

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Cache {
    client_info: ClientInformation,
    sessions: HashMap<String, CredentialsWrapper>,
}

pub trait CacheManager {
    type Error: 'static + std::fmt::Debug + std::error::Error;

    fn load_cache(&mut self) -> Result<(), Self::Error>;
    fn commit(&self) -> Result<(), Self::Error>;
    fn get_cache_as_ref(&self) -> &Cache;
    fn get_cache_as_mut(&mut self) -> &mut Cache;
    fn get_cache(&self) -> Cache;

    fn is_valid(&self, start_url: &str) -> bool {
        self.get_cache_as_ref()
            .client_info
            .start_url
            .as_ref()
            .map_or(false, |cache_start_url| start_url == cache_start_url)
    }

    fn get_access_token(&self) -> Option<&str> {
        let ci = &self.get_cache_as_ref().client_info;
        match (&ci.access_token, &ci.access_token_expires_at) {
            (Some(access_token), Some(expires_at)) => {
                let now = Utc::now();
                let expiration_time = *expires_at - EXPIRATION_BUFFER;
                if now < expiration_time {
                    Some(access_token)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn get_refresh_token(&self) -> Option<&str> {
        self.get_client_credentials()?;
        self.get_cache_as_ref().client_info.refresh_token.as_deref()
    }

    fn get_client_credentials(&self) -> Option<(&str, &str)> {
        let ci = &self.get_cache_as_ref().client_info;
        match (
            &ci.client_id,
            &ci.client_secret,
            &ci.client_secret_expires_at,
        ) {
            (Some(client_id), Some(client_secret), Some(expires_at)) => {
                let now = Utc::now();
                let expiration_time = *expires_at - EXPIRATION_BUFFER;
                if now < expiration_time {
                    Some((client_id, client_secret))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn get_session(&self, account_id: &str, role_name: &str) -> Option<&CredentialsWrapper> {
        let cache_key = format!("{}-{}", account_id, role_name);
        let credentials = self.get_cache_as_ref().sessions.get(&cache_key)?;

        if let Some(expiry) = credentials.expires_after {
            if Utc::now() > expiry + EXPIRATION_BUFFER {
                return None;
            }
        }

        Some(credentials)
    }

    fn get_client_information(&self) -> ClientInformation {
        self.get_cache().client_info
    }

    fn set_client(
        &mut self,
        client_id: String,
        client_secret: String,
        client_secret_expires_at: i64,
    ) {
        self.get_cache_as_mut().client_info.client_id = Some(client_id);
        self.get_cache_as_mut().client_info.client_secret = Some(client_secret);
        self.get_cache_as_mut().client_info.client_secret_expires_at =
            DateTime::from_timestamp(client_secret_expires_at, 0);
    }

    fn set_access_token(&mut self, access_token: String, access_token_expires_in: i32) {
        self.get_cache_as_mut().client_info.access_token = Some(access_token);
        self.get_cache_as_mut().client_info.access_token_expires_at =
            Some(Utc::now() + Duration::seconds(access_token_expires_in as i64));
    }

    fn set_session(&mut self, account_id: &str, role_name: &str, credentials: Credentials) {
        self.get_cache_as_mut().sessions.insert(
            format!("{}-{}", account_id, role_name),
            CredentialsWrapper::from(credentials),
        );
    }

    fn set_client_info(&mut self, client_info: ClientInformation) {
        self.get_cache_as_mut().client_info = client_info;
    }
}

pub struct MonoJsonCacheManager {
    
}