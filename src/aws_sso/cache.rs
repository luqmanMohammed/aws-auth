use crate::aws_sso::config::{AwsSsoConfig, CacheBackend};
use crate::aws_sso::types::{ClientInformation, CredentialsWrapper};

use aws_sdk_ssooidc::config::Credentials;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub mod mono_json;
pub mod sealed_json;

const EXPIRATION_BUFFER: SignedDuration = SignedDuration::from_mins(5);

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Cache {
    client_info: ClientInformation,
    sessions: HashMap<String, CredentialsWrapper>,
}

pub enum CacheRefMut<'a, C: ManageCache> {
    Owned(C),
    BorrowedMut(&'a mut C),
}

impl<C: ManageCache> ManageCache for CacheRefMut<'_, C> {
    type Error = C::Error;

    fn load_cache(&mut self) -> Result<(), Self::Error> {
        match self {
            CacheRefMut::Owned(c) => c.load_cache(),
            CacheRefMut::BorrowedMut(c) => c.load_cache(),
        }
    }

    fn commit(&self) -> Result<(), Self::Error> {
        match self {
            CacheRefMut::Owned(c) => c.commit(),
            CacheRefMut::BorrowedMut(c) => c.commit(),
        }
    }

    fn get_cache_as_ref(&self) -> &Cache {
        match self {
            CacheRefMut::Owned(c) => c.get_cache_as_ref(),
            CacheRefMut::BorrowedMut(c) => c.get_cache_as_ref(),
        }
    }

    fn get_cache_as_mut(&mut self) -> &mut Cache {
        match self {
            CacheRefMut::Owned(c) => c.get_cache_as_mut(),
            CacheRefMut::BorrowedMut(c) => c.get_cache_as_mut(),
        }
    }
}

impl<C: ManageCache> From<C> for CacheRefMut<'_, C> {
    fn from(cache_manager: C) -> Self {
        CacheRefMut::Owned(cache_manager)
    }
}

impl<'a, C: ManageCache> From<&'a mut C> for CacheRefMut<'a, C> {
    fn from(cache_manager: &'a mut C) -> Self {
        CacheRefMut::BorrowedMut(cache_manager)
    }
}

pub trait ManageCache {
    type Error: std::error::Error;

    /// A missing or corrupt cache loads as if there were none. An error means the store itself
    /// cannot be used, so nothing obtained by signing in now could be kept.
    fn load_cache(&mut self) -> Result<(), Self::Error>;
    fn commit(&self) -> Result<(), Self::Error>;
    fn get_cache_as_ref(&self) -> &Cache;
    fn get_cache_as_mut(&mut self) -> &mut Cache;

    fn is_valid(&self, start_url: &str) -> bool {
        self.get_cache_as_ref()
            .client_info
            .start_url
            .as_ref()
            .is_some_and(|cache_start_url| start_url == cache_start_url)
    }

    fn get_access_token(&self) -> Option<&str> {
        let ci = &self.get_cache_as_ref().client_info;
        match (&ci.access_token, &ci.access_token_expires_at) {
            (Some(access_token), Some(expires_at)) => {
                let now = Timestamp::now();
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
                let now = Timestamp::now();
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
        let expiry = credentials.expires_after?;
        if Timestamp::now() > expiry - EXPIRATION_BUFFER {
            return None;
        }

        Some(credentials)
    }

    #[allow(dead_code)]
    fn set_client(
        &mut self,
        client_id: String,
        client_secret: String,
        client_secret_expires_at: i64,
    ) {
        self.get_cache_as_mut().client_info.client_id = Some(client_id);
        self.get_cache_as_mut().client_info.client_secret = Some(client_secret);
        self.get_cache_as_mut().client_info.client_secret_expires_at =
            Timestamp::from_second(client_secret_expires_at).ok();
    }

    #[allow(dead_code)]
    fn set_access_token(&mut self, access_token: String, access_token_expires_in: i32) {
        self.get_cache_as_mut().client_info.access_token = Some(access_token);
        self.get_cache_as_mut().client_info.access_token_expires_at =
            Some(Timestamp::now() + SignedDuration::from_secs(access_token_expires_in.into()));
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

    fn clear_sessions(&mut self) {
        self.get_cache_as_mut().sessions = HashMap::new();
    }

    fn get_computed_client_info(&self) -> ClientInformation {
        let mut ninfo = ClientInformation::default();
        let cinfo = self.get_cache_as_ref().client_info.clone();

        if self.get_client_credentials().is_none() {
            return ninfo;
        }
        ninfo.client_id = cinfo.client_id;
        ninfo.client_secret = cinfo.client_secret;
        ninfo.client_secret_expires_at = cinfo.client_secret_expires_at;

        // Carried even when the access token has expired -- that is precisely when it is needed,
        // to renew without another device authorization.
        if self.get_refresh_token().is_some() {
            ninfo.refresh_token = cinfo.refresh_token;
        }

        if self.get_access_token().is_some() {
            ninfo.access_token = cinfo.access_token;
            ninfo.access_token_expires_at = cinfo.access_token_expires_at;
        }

        ninfo
    }

    fn cache_reset(&mut self) {
        self.get_cache_as_mut().client_info = ClientInformation::default();
        self.get_cache_as_mut().sessions = HashMap::new();
    }
}

pub enum CacheStore {
    File(mono_json::MonoJsonCacheManager),
    Keyring(sealed_json::SealedJsonCacheManager),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    File(#[from] mono_json::Error),
    #[error(transparent)]
    Keyring(#[from] sealed_json::Error),
}

impl CacheStore {
    pub fn from_config(config: &AwsSsoConfig, cache_dir: &Path) -> Self {
        match config.cache_backend() {
            CacheBackend::File => Self::File(mono_json::MonoJsonCacheManager::new(cache_dir)),
            CacheBackend::Keyring => {
                Self::Keyring(sealed_json::SealedJsonCacheManager::new(cache_dir))
            }
        }
    }
}

impl ManageCache for CacheStore {
    type Error = Error;

    fn load_cache(&mut self) -> Result<(), Self::Error> {
        match self {
            CacheStore::File(c) => Ok(c.load_cache()?),
            CacheStore::Keyring(c) => Ok(c.load_cache()?),
        }
    }

    fn commit(&self) -> Result<(), Self::Error> {
        match self {
            CacheStore::File(c) => Ok(c.commit()?),
            CacheStore::Keyring(c) => Ok(c.commit()?),
        }
    }

    fn get_cache_as_ref(&self) -> &Cache {
        match self {
            CacheStore::File(c) => c.get_cache_as_ref(),
            CacheStore::Keyring(c) => c.get_cache_as_ref(),
        }
    }

    fn get_cache_as_mut(&mut self) -> &mut Cache {
        match self {
            CacheStore::File(c) => c.get_cache_as_mut(),
            CacheStore::Keyring(c) => c.get_cache_as_mut(),
        }
    }
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;

    struct TestCache {
        cache: Cache,
    }

    impl ManageCache for TestCache {
        type Error = std::io::Error;
        fn load_cache(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn commit(&self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn get_cache_as_ref(&self) -> &Cache {
            &self.cache
        }
        fn get_cache_as_mut(&mut self) -> &mut Cache {
            &mut self.cache
        }
    }

    fn in_hours(hours: i64) -> Timestamp {
        Timestamp::now() + SignedDuration::from_hours(hours)
    }

    /// A cache with a live client registration and, unless overridden, live tokens.
    fn cache_with(
        client_secret_expiry: Timestamp,
        access_token_expiry: Timestamp,
        refresh_token: Option<&str>,
    ) -> TestCache {
        TestCache {
            cache: Cache {
                client_info: ClientInformation {
                    start_url: Some("https://example.awsapps.com/start".to_string()),
                    client_id: Some("client-id".to_string()),
                    client_secret: Some("client-secret".to_string()),
                    client_secret_expires_at: Some(client_secret_expiry),
                    access_token: Some("access-token".to_string()),
                    access_token_expires_at: Some(access_token_expiry),
                    refresh_token: refresh_token.map(str::to_string),
                },
                sessions: HashMap::new(),
            },
        }
    }

    #[test]
    fn a_live_cache_carries_everything() {
        let cache = cache_with(in_hours(24), in_hours(1), Some("refresh-token"));

        let info = cache.get_computed_client_info();

        assert_eq!(info.client_id.as_deref(), Some("client-id"));
        assert_eq!(info.access_token.as_deref(), Some("access-token"));
        assert_eq!(info.refresh_token.as_deref(), Some("refresh-token"));
    }

    #[test]
    fn an_expired_access_token_still_carries_the_refresh_token() {
        // Dropping it here is what forced a browser login on every token expiry.
        let cache = cache_with(in_hours(24), in_hours(-1), Some("refresh-token"));

        let info = cache.get_computed_client_info();

        assert_eq!(info.client_id.as_deref(), Some("client-id"));
        assert!(info.access_token.is_none(), "the expired token is dropped");
        assert_eq!(
            info.refresh_token.as_deref(),
            Some("refresh-token"),
            "the refresh token is exactly what is needed now"
        );
    }

    #[test]
    fn an_access_token_inside_the_expiry_buffer_counts_as_expired() {
        let cache = cache_with(
            in_hours(24),
            Timestamp::now() + SignedDuration::from_mins(1),
            None,
        );

        assert!(
            cache.get_access_token().is_none(),
            "a token expiring within the buffer should not be used"
        );
    }

    #[test]
    fn an_access_token_beyond_the_buffer_is_used() {
        let cache = cache_with(
            in_hours(24),
            Timestamp::now() + SignedDuration::from_mins(30),
            None,
        );

        assert_eq!(cache.get_access_token(), Some("access-token"));
    }

    #[test]
    fn an_expired_client_secret_invalidates_everything() {
        let cache = cache_with(in_hours(-1), in_hours(1), Some("refresh-token"));

        let info = cache.get_computed_client_info();

        assert!(info.client_id.is_none(), "re-registration is required");
        assert!(info.access_token.is_none());
        assert!(info.refresh_token.is_none());
    }

    #[test]
    fn a_cache_for_another_start_url_is_not_valid() {
        let cache = cache_with(in_hours(24), in_hours(1), None);

        assert!(cache.is_valid("https://example.awsapps.com/start"));
        assert!(!cache.is_valid("https://other.awsapps.com/start"));
    }

    #[test]
    fn sessions_are_returned_per_account_and_role() {
        let mut cache = cache_with(in_hours(24), in_hours(1), None);
        cache.get_cache_as_mut().sessions.insert(
            "111111111111-Admin".to_string(),
            CredentialsWrapper {
                access_key_id: "AKIA".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
                expires_after: Some(in_hours(1)),
            },
        );

        assert!(cache.get_session("111111111111", "Admin").is_some());
        assert!(
            cache.get_session("111111111111", "Other").is_none(),
            "a different role must not share a session"
        );
        assert!(
            cache.get_session("222222222222", "Admin").is_none(),
            "a different account must not share a session"
        );
    }

    #[test]
    fn an_expired_session_is_not_returned() {
        let mut cache = cache_with(in_hours(24), in_hours(1), None);
        cache.get_cache_as_mut().sessions.insert(
            "111111111111-Admin".to_string(),
            CredentialsWrapper {
                access_key_id: "AKIA".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
                expires_after: Some(Timestamp::now() + SignedDuration::from_mins(1)),
            },
        );

        assert!(
            cache.get_session("111111111111", "Admin").is_none(),
            "a session inside the expiry buffer should not be reused"
        );
    }

    #[test]
    fn clearing_sessions_leaves_the_client_registration() {
        let mut cache = cache_with(in_hours(24), in_hours(1), Some("refresh-token"));
        cache.get_cache_as_mut().sessions.insert(
            "111111111111-Admin".to_string(),
            CredentialsWrapper {
                access_key_id: "AKIA".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
                expires_after: Some(in_hours(1)),
            },
        );

        cache.clear_sessions();

        assert!(cache.get_session("111111111111", "Admin").is_none());
        assert_eq!(cache.get_access_token(), Some("access-token"));
    }

    #[test]
    fn resetting_clears_the_registration_too() {
        let mut cache = cache_with(in_hours(24), in_hours(1), Some("refresh-token"));

        cache.cache_reset();

        assert!(cache.get_access_token().is_none());
        assert!(cache.get_client_credentials().is_none());
    }

    #[test]
    fn a_file_store_round_trips_through_its_directory() {
        let dir = crate::utils::test_support::TempDir::new("cache-store-file");
        let mut config = crate::aws_sso::config::UnverifiedSsoConfig::new(
            "https://example.awsapps.com/start".to_string(),
            "eu-west-2".to_string(),
        );
        config.cache_backend = Some(CacheBackend::File);
        let config = config.verify().unwrap();
        let mut written = CacheStore::from_config(&config, dir.path());
        written.set_access_token("access-token".to_string(), 3600);

        written.commit().expect("commit should succeed");
        let mut read = CacheStore::from_config(&config, dir.path());
        read.load_cache().expect("load should succeed");

        assert!(matches!(read, CacheStore::File(_)));
        assert!(dir.join("cache.json").exists());
        assert_eq!(
            read.get_cache_as_ref().client_info.access_token.as_deref(),
            Some("access-token")
        );
    }
}
