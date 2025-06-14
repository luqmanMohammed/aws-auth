use crate::aws_sso::types::{ClientInformation, CredentialsWrapper};

use aws_sdk_ssooidc::config::Credentials;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const EXPIRATION_BUFFER: Duration = Duration::minutes(5);

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
            CacheRefMut::Owned(ref mut c) => c.load_cache(),
            CacheRefMut::BorrowedMut(c) => c.load_cache(),
        }
    }

    fn commit(&self) -> Result<(), Self::Error> {
        match self {
            CacheRefMut::Owned(ref c) => c.commit(),
            CacheRefMut::BorrowedMut(c) => c.commit(),
        }
    }

    fn get_cache_as_ref(&self) -> &Cache {
        match self {
            CacheRefMut::Owned(ref c) => c.get_cache_as_ref(),
            CacheRefMut::BorrowedMut(c) => c.get_cache_as_ref(),
        }
    }

    fn get_cache_as_mut(&mut self) -> &mut Cache {
        match self {
            CacheRefMut::Owned(ref mut c) => c.get_cache_as_mut(),
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
    type Error: 'static + std::fmt::Debug + std::error::Error;

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
            if Utc::now() > expiry - EXPIRATION_BUFFER {
                return None;
            }
        } else {
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
            DateTime::from_timestamp(client_secret_expires_at, 0);
    }

    #[allow(dead_code)]
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

    fn clear_sessions(&mut self) {
        self.get_cache_as_mut().sessions = HashMap::new();
    }

    fn get_computed_client_info(&self) -> ClientInformation {
        let mut ninfo = ClientInformation::default();
        let cinfo = self.get_cache_as_ref().client_info.clone();

        if self.get_client_credentials().is_some() {
            ninfo.client_id = cinfo.client_id;
            ninfo.client_secret = cinfo.client_secret;
            ninfo.client_secret_expires_at = cinfo.client_secret_expires_at;
        } else {
            return ninfo;
        }

        if self.get_access_token().is_some() {
            ninfo.access_token = cinfo.access_token;
            ninfo.access_token_expires_at = cinfo.access_token_expires_at;
        } else {
            return ninfo;
        }

        if self.get_refresh_token().is_some() {
            ninfo.refresh_token = cinfo.refresh_token;
        }

        ninfo
    }

    fn cache_reset(&mut self) {
        self.get_cache_as_mut().client_info = ClientInformation::default();
        self.get_cache_as_mut().sessions = HashMap::new();
    }
}

pub mod mono_json {
    use crate::aws_sso::cache::Cache;
    use crate::aws_sso::cache::ManageCache;
    use std::fs::File;
    use std::path::{Path, PathBuf};

    #[derive(Debug)]
    pub enum Error {
        SerdeJson(serde_json::Error),
        CacheNotFound(std::io::Error),
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Error::SerdeJson(err) => writeln!(f, "Invalid cache json: {}", err),
                Error::CacheNotFound(err) => writeln!(f, "Cache not found: {}", err),
            }
        }
    }

    impl std::error::Error for Error {}

    pub struct MonoJsonCacheManager {
        cache: Cache,
        cache_path: PathBuf,
    }

    impl MonoJsonCacheManager {
        pub fn new(cache_dir: &Path) -> Self {
            Self {
                cache: Cache::default(),
                cache_path: cache_dir.join("cache.json"),
            }
        }
    }

    impl ManageCache for MonoJsonCacheManager {
        type Error = Error;

        fn load_cache(&mut self) -> Result<(), Self::Error> {
            let cache_file = File::open(&self.cache_path).map_err(Error::CacheNotFound)?;
            let cache =
                serde_json::from_reader::<File, Cache>(cache_file).map_err(Error::SerdeJson)?;
            self.cache = cache;
            Ok(())
        }

        fn commit(&self) -> Result<(), Self::Error> {
            let cache_file = File::create(&self.cache_path).map_err(Error::CacheNotFound)?;
            serde_json::to_writer(cache_file, &self.cache).map_err(Error::SerdeJson)?;
            Ok(())
        }

        fn get_cache_as_ref(&self) -> &Cache {
            &self.cache
        }

        fn get_cache_as_mut(&mut self) -> &mut Cache {
            &mut self.cache
        }
    }
}
