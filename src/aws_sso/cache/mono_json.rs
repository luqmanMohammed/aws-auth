use crate::aws_sso::cache::Cache;
use crate::aws_sso::cache::ManageCache;
use crate::utils::private_fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error serializing cache json: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Error accessing cache file: {0}")]
    Io(#[from] io::Error),
}

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
        self.cache = match std::fs::read(&self.cache_path) {
            Ok(contents) => serde_json::from_slice(&contents).unwrap_or_default(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Cache::default(),
            Err(err) => return Err(err.into()),
        };
        Ok(())
    }

    fn commit(&self) -> Result<(), Self::Error> {
        let cache = serde_json::to_vec(&self.cache)?;
        private_fs::write_atomic(&self.cache_path, &cache)?;
        Ok(())
    }

    fn get_cache_as_ref(&self) -> &Cache {
        &self.cache
    }

    fn get_cache_as_mut(&mut self) -> &mut Cache {
        &mut self.cache
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;

    #[test]
    fn a_missing_cache_loads_as_empty() {
        let dir = TempDir::new("mono-missing");
        let mut cache = MonoJsonCacheManager::new(dir.path());

        cache.load_cache().expect("a missing cache is not an error");

        assert!(cache.get_access_token().is_none());
    }

    #[test]
    fn a_corrupt_cache_loads_as_empty() {
        let dir = TempDir::new("mono-corrupt");
        std::fs::write(dir.join("cache.json"), b"{not json").unwrap();
        let mut cache = MonoJsonCacheManager::new(dir.path());

        cache.load_cache().expect("a corrupt cache is not an error");

        assert!(cache.get_access_token().is_none());
    }

    #[test]
    fn an_unreadable_cache_is_an_error() {
        let dir = TempDir::new("mono-unreadable");
        std::fs::create_dir(dir.join("cache.json")).unwrap();
        let mut cache = MonoJsonCacheManager::new(dir.path());

        assert!(matches!(cache.load_cache(), Err(Error::Io(_))));
    }
}
