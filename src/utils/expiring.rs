use crate::utils::private_fs;
use jiff::{SignedDuration, Timestamp};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io;
use std::marker::PhantomData;
use std::path::PathBuf;

#[derive(Deserialize)]
struct UncheckedExpiring<T> {
    data: T,
    expires_at: Option<Timestamp>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    try_from = "UncheckedExpiring<T>",
    bound(deserialize = "T: Deserialize<'de>")
)]
pub struct Expiring<T> {
    data: T,
    expires_at: Option<Timestamp>,
}

impl<T> Expiring<T> {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= Timestamp::now())
    }

    pub fn until(data: T, expires_at: Option<Timestamp>) -> Option<Self> {
        let expiring = Self { data, expires_at };
        (!expiring.is_expired()).then_some(expiring)
    }

    /// A deadline beyond jiff's range never arrives, so it is kept as no deadline at all.
    pub fn new(data: T, ttl: SignedDuration) -> Option<Self> {
        Self::until(data, Timestamp::now().checked_add(ttl).ok())
    }

    pub fn data(&self) -> &T {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    pub fn into_data(self) -> T {
        self.data
    }
}

impl<T> TryFrom<UncheckedExpiring<T>> for Expiring<T> {
    type Error = &'static str;

    fn try_from(unchecked: UncheckedExpiring<T>) -> Result<Self, Self::Error> {
        Self::until(unchecked.data, unchecked.expires_at).ok_or("expired")
    }
}

pub struct ExpiringFile<T> {
    path: PathBuf,
    _data: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned> ExpiringFile<T> {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            _data: PhantomData,
        }
    }

    // Read without checking for existence first so a file removed underneath us is the same
    // case as one that was never there.
    pub fn load(&self) -> io::Result<Option<Expiring<T>>> {
        match std::fs::read(&self.path) {
            Ok(contents) => Ok(serde_json::from_slice(&contents).ok()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub fn store(&self, value: &Expiring<T>) -> io::Result<()> {
        private_fs::write_atomic(&self.path, &serde_json::to_vec(value)?)
    }

    pub fn clear(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;

    fn file(dir: &TempDir) -> ExpiringFile<u32> {
        ExpiringFile::new(dir.join("e.json"))
    }

    fn past() -> Timestamp {
        Timestamp::now() - SignedDuration::from_secs(1)
    }

    fn future() -> Timestamp {
        Timestamp::now() + SignedDuration::from_hours(1)
    }

    #[test]
    fn a_deadline_in_the_past_cannot_be_built() {
        assert!(Expiring::until(1, Some(past())).is_none());
        assert!(Expiring::new(1, SignedDuration::from_secs(-1)).is_none());
        assert!(Expiring::new(1, SignedDuration::ZERO).is_none());
    }

    #[test]
    fn a_future_or_absent_deadline_can_be_built() {
        assert!(Expiring::until(1, Some(future())).is_some());
        assert!(Expiring::until(1, None).is_some());
        assert!(Expiring::new(1, SignedDuration::from_hours(1)).is_some());
    }

    #[test]
    fn a_ttl_beyond_the_representable_range_never_expires() {
        let expiring = Expiring::new(1, SignedDuration::MAX).expect("should be live");
        assert_eq!(expiring.expires_at, None);
    }

    #[test]
    fn an_expired_envelope_does_not_deserialize() {
        let json = format!(r#"{{"data":7,"expires_at":"{}"}}"#, past());
        assert!(serde_json::from_str::<Expiring<u32>>(&json).is_err());
    }

    #[test]
    fn a_stored_value_loads_back_with_its_deadline() {
        let dir = TempDir::new("expiring-roundtrip");
        let deadline = future();
        file(&dir)
            .store(&Expiring::until(7, Some(deadline)).unwrap())
            .unwrap();

        let loaded = file(&dir).load().unwrap().expect("should be live");

        assert_eq!(*loaded.data(), 7);
        assert_eq!(loaded.expires_at, Some(deadline));
    }

    #[test]
    fn a_missing_file_loads_as_none() {
        let dir = TempDir::new("expiring-missing");
        assert!(file(&dir).load().unwrap().is_none());
    }

    #[test]
    fn a_corrupt_file_loads_as_none() {
        let dir = TempDir::new("expiring-corrupt");
        std::fs::write(dir.join("e.json"), b"not json").unwrap();
        assert!(file(&dir).load().unwrap().is_none());
    }

    #[test]
    fn an_expired_file_loads_as_none() {
        let dir = TempDir::new("expiring-expired");
        let expired = format!(r#"{{"data":7,"expires_at":"{}"}}"#, past());
        std::fs::write(dir.join("e.json"), expired).unwrap();
        assert!(file(&dir).load().unwrap().is_none());
    }

    #[test]
    fn clearing_removes_the_file_and_tolerates_a_missing_one() {
        let dir = TempDir::new("expiring-clear");
        file(&dir)
            .store(&Expiring::until(1, None).unwrap())
            .unwrap();

        file(&dir).clear().expect("the file should be removed");
        assert!(!dir.join("e.json").exists());
        file(&dir).clear().expect("a missing file is not an error");
    }
}
