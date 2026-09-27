use crate::aws_sso::cache::Cache;
use crate::aws_sso::cache::ManageCache;
use crate::utils::private_fs;
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::XNonce;
use chacha20poly1305::aead::{Aead, Generate, KeyInit};
use std::cell::OnceCell;
use std::io;
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "aws-auth";
const NONCE_LEN: usize = 24;

type Key = chacha20poly1305::Key;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "OS keyring unavailable ({0}); set \"cacheBackend\" to \"file\" in config.json to use aws-auth on this machine"
    )]
    KeystoreUnavailable(keyring::Error),
    #[error("Error storing the cache key in the OS keyring: {0}")]
    Keyring(keyring::Error),
    #[error("Cache file is too short to be a sealed cache")]
    MalformedFile,
    #[error("Error encrypting the cache")]
    Encrypt,
    #[error("Cache could not be decrypted with the key in the OS keyring")]
    Decrypt,
    #[error("Error serializing cache json: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Error accessing cache file: {0}")]
    Io(#[from] io::Error),
}

pub struct SealedJsonCacheManager {
    cache: Cache,
    cache_path: PathBuf,
    account: String,
    entry: OnceCell<keyring::Entry>,
    key: OnceCell<Key>,
}

impl SealedJsonCacheManager {
    pub fn new(cache_dir: &Path) -> Self {
        // Keyed by directory because the keyring is machine-wide: a scratch `--config-dir`
        // must never read or replace the key of the real one.
        let account = std::fs::canonicalize(cache_dir).unwrap_or_else(|_| cache_dir.into());
        Self {
            cache: Cache::default(),
            cache_path: cache_dir.join("cache.sealed"),
            account: account.to_string_lossy().into_owned(),
            entry: OnceCell::new(),
            key: OnceCell::new(),
        }
    }

    fn entry(&self) -> Result<&keyring::Entry, Error> {
        if let Some(entry) = self.entry.get() {
            return Ok(entry);
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, &self.account)
            .map_err(Error::KeystoreUnavailable)?;
        Ok(self.entry.get_or_init(|| entry))
    }

    fn stored_key(&self) -> Result<Option<Key>, Error> {
        match self.entry()?.get_secret() {
            Ok(secret) => Ok(Key::try_from(secret.as_slice()).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(Error::KeystoreUnavailable(err)),
        }
    }

    fn key(&self) -> Result<&Key, Error> {
        if let Some(key) = self.key.get() {
            return Ok(key);
        }
        let key = match self.stored_key()? {
            Some(key) => key,
            None => {
                let key = Key::generate();
                self.entry()?.set_secret(&key).map_err(Error::Keyring)?;
                key
            }
        };
        Ok(self.key.get_or_init(|| key))
    }
}

impl ManageCache for SealedJsonCacheManager {
    type Error = Error;

    fn load_cache(&mut self) -> Result<(), Self::Error> {
        self.cache = Cache::default();
        // The keyring is reached even with no file to open, so an unreachable one fails before
        // a sign-in whose result could never be stored.
        let Some(key) = self.stored_key()? else {
            return Ok(());
        };
        let key = self.key.get_or_init(|| key);
        let sealed = match std::fs::read(&self.cache_path) {
            Ok(sealed) => sealed,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let cache = match unseal(key, &sealed) {
            Ok(plaintext) => serde_json::from_slice(&plaintext).ok(),
            Err(err) => {
                eprintln!("WARN: Treating {:?} as no cache: {err}", self.cache_path);
                None
            }
        };
        self.cache = cache.unwrap_or_default();
        Ok(())
    }

    fn commit(&self) -> Result<(), Self::Error> {
        let sealed = seal(self.key()?, &serde_json::to_vec(&self.cache)?)?;
        private_fs::write_atomic(&self.cache_path, &sealed)?;
        Ok(())
    }

    fn get_cache_as_ref(&self) -> &Cache {
        &self.cache
    }

    fn get_cache_as_mut(&mut self) -> &mut Cache {
        &mut self.cache
    }
}

fn seal(key: &Key, plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    let nonce = XNonce::generate();
    let ciphertext = XChaCha20Poly1305::new(key)
        .encrypt(&nonce, plaintext)
        .map_err(|_| Error::Encrypt)?;
    Ok([nonce.as_slice(), &ciphertext].concat())
}

fn unseal(key: &Key, sealed: &[u8]) -> Result<Vec<u8>, Error> {
    let (nonce, ciphertext) = sealed
        .split_at_checked(NONCE_LEN)
        .ok_or(Error::MalformedFile)?;
    let nonce = XNonce::try_from(nonce).map_err(|_| Error::MalformedFile)?;
    XChaCha20Poly1305::new(key)
        .decrypt(&nonce, ciphertext)
        .map_err(|_| Error::Decrypt)
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_cache_unseals_to_the_original() {
        let key = Key::generate();

        let sealed = seal(&key, b"{\"cache\":true}").unwrap();

        assert_eq!(unseal(&key, &sealed).unwrap(), b"{\"cache\":true}");
    }

    #[test]
    fn the_plaintext_does_not_appear_in_the_sealed_bytes() {
        let key = Key::generate();

        let sealed = seal(&key, b"refresh-token").unwrap();

        assert!(
            !sealed
                .windows(b"refresh-token".len())
                .any(|window| window == b"refresh-token")
        );
    }

    #[test]
    fn sealing_twice_uses_a_fresh_nonce() {
        let key = Key::generate();

        let first = seal(&key, b"same").unwrap();
        let second = seal(&key, b"same").unwrap();

        assert_ne!(first, second, "a reused nonce would leak the plaintext xor");
    }

    #[test]
    fn a_tampered_byte_fails_to_unseal() {
        let key = Key::generate();
        let mut sealed = seal(&key, b"payload").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;

        assert!(matches!(unseal(&key, &sealed), Err(Error::Decrypt)));
    }

    #[test]
    fn another_key_fails_to_unseal() {
        let sealed = seal(&Key::generate(), b"payload").unwrap();

        assert!(matches!(
            unseal(&Key::generate(), &sealed),
            Err(Error::Decrypt)
        ));
    }

    #[test]
    fn a_file_shorter_than_the_nonce_is_malformed() {
        let key = Key::generate();

        assert!(matches!(
            unseal(&key, &[0; NONCE_LEN - 1]),
            Err(Error::MalformedFile)
        ));
    }

    #[test]
    fn a_plaintext_json_cache_is_not_mistaken_for_a_sealed_one() {
        let key = Key::generate();
        let plaintext = serde_json::to_vec(&Cache::default()).unwrap();

        assert!(unseal(&key, &plaintext).is_err());
    }
}
