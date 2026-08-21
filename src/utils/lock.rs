use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct CounterLock {
    threshold: u64,
    count: u64,
    locked_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl CounterLock {
    pub fn is_locked(&self) -> bool {
        self.locked_at.is_some()
    }
    pub fn increment(&mut self, count: u64) {
        self.count += count;
        if self.count >= self.threshold {
            self.locked_at = Some(chrono::Utc::now());
        }
    }
    pub fn reset(&mut self) {
        self.count = 0;
        self.locked_at = None;
    }
}

pub trait CounterLockProvider {
    type Error: std::error::Error;
    fn load_lock(&mut self) -> Result<(), Self::Error>;
    fn save_lock(&self) -> Result<(), Self::Error>;
    fn get_lock(&self) -> &CounterLock;
    fn get_lock_mut(&mut self) -> &mut CounterLock;
}

pub struct DecayingJsonCounterLockProvider {
    lock_path: PathBuf,
    lock: Option<CounterLock>,
    threshold: u64,
    lock_decay_duration: Option<chrono::Duration>,
}

impl DecayingJsonCounterLockProvider {
    pub fn new(
        base_dir: &Path,
        lockname: &str,
        threshold: u64,
        lock_decay_duration: Option<chrono::Duration>,
    ) -> Self {
        Self {
            lock_path: base_dir.join(lockname).with_extension("json"),
            lock: None,
            threshold,
            lock_decay_duration,
        }
    }
}

impl CounterLockProvider for DecayingJsonCounterLockProvider {
    type Error = std::io::Error;

    fn load_lock(&mut self) -> Result<(), Self::Error> {
        let lock_path = &self.lock_path;
        if lock_path.exists() {
            let file = std::fs::File::open(lock_path)?;
            let mut lock: CounterLock = serde_json::from_reader(file)?;
            let mut save_lock = false;
            if let Some((ldd, la)) = self.lock_decay_duration.zip(lock.locked_at)
                && Utc::now() >= la + ldd
            {
                lock = CounterLock {
                    threshold: self.threshold,
                    count: 0,
                    locked_at: None,
                };
                save_lock = true;
            }
            lock.threshold = self.threshold;
            self.lock = Some(lock);
            if save_lock {
                self.save_lock()?
            }
        } else {
            self.lock = Some(CounterLock {
                threshold: self.threshold,
                count: 0,
                locked_at: None,
            });
        }
        Ok(())
    }

    fn save_lock(&self) -> Result<(), Self::Error> {
        if let Some(ref lock) = self.lock {
            let file = std::fs::File::create(&self.lock_path)?;
            serde_json::to_writer(file, lock)?;
        }
        Ok(())
    }

    fn get_lock(&self) -> &CounterLock {
        self.lock.as_ref().expect("Make sure lock is loaded")
    }

    fn get_lock_mut(&mut self) -> &mut CounterLock {
        self.lock.as_mut().expect("Make sure lock is loaded")
    }
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;

    fn unlocked(threshold: u64) -> CounterLock {
        CounterLock {
            threshold,
            count: 0,
            locked_at: None,
        }
    }

    #[test]
    fn locks_once_the_threshold_is_reached() {
        let mut lock = unlocked(3);
        lock.increment(1);
        assert!(!lock.is_locked(), "one of three");
        lock.increment(1);
        assert!(!lock.is_locked(), "two of three");
        lock.increment(1);
        assert!(lock.is_locked(), "three of three should lock");
    }

    #[test]
    fn a_larger_increment_can_cross_the_threshold_at_once() {
        let mut lock = unlocked(3);
        lock.increment(5);
        assert!(lock.is_locked());
    }

    #[test]
    fn resetting_clears_both_the_count_and_the_lock() {
        let mut lock = unlocked(1);
        lock.increment(1);
        assert!(lock.is_locked(), "precondition");

        lock.reset();

        assert!(!lock.is_locked());
        lock.increment(1);
        assert!(lock.is_locked(), "counting restarts from zero");
    }

    #[test]
    fn a_missing_lock_file_loads_as_unlocked() {
        let dir = TempDir::new("lock-missing");
        let mut provider = DecayingJsonCounterLockProvider::new(dir.path(), "l", 2, None);

        provider
            .load_lock()
            .expect("a missing file is not an error");

        assert!(!provider.get_lock().is_locked());
    }

    #[test]
    fn a_lock_survives_a_save_and_load() {
        let dir = TempDir::new("lock-roundtrip");
        let mut provider = DecayingJsonCounterLockProvider::new(dir.path(), "l", 2, None);
        provider.load_lock().unwrap();
        provider.get_lock_mut().increment(2);
        provider.save_lock().unwrap();

        let mut reloaded = DecayingJsonCounterLockProvider::new(dir.path(), "l", 2, None);
        reloaded.load_lock().unwrap();

        assert!(reloaded.get_lock().is_locked(), "the lock should persist");
    }

    #[test]
    fn a_lock_older_than_the_decay_window_clears_on_load() {
        let dir = TempDir::new("lock-decayed");
        let mut provider = DecayingJsonCounterLockProvider::new(dir.path(), "l", 1, None);
        provider.load_lock().unwrap();
        provider.get_lock_mut().increment(1);
        provider.save_lock().unwrap();
        assert!(provider.get_lock().is_locked(), "precondition");

        // Same file, but now read by a provider that expires locks after a second.
        let mut decaying = DecayingJsonCounterLockProvider::new(
            dir.path(),
            "l",
            1,
            Some(chrono::Duration::seconds(-1)),
        );
        decaying.load_lock().unwrap();

        assert!(
            !decaying.get_lock().is_locked(),
            "the lock should have decayed"
        );
    }

    #[test]
    fn a_lock_inside_the_decay_window_is_kept() {
        let dir = TempDir::new("lock-fresh");
        let mut provider = DecayingJsonCounterLockProvider::new(dir.path(), "l", 1, None);
        provider.load_lock().unwrap();
        provider.get_lock_mut().increment(1);
        provider.save_lock().unwrap();

        let mut still_locked = DecayingJsonCounterLockProvider::new(
            dir.path(),
            "l",
            1,
            Some(chrono::Duration::hours(2)),
        );
        still_locked.load_lock().unwrap();

        assert!(still_locked.get_lock().is_locked());
    }

    #[test]
    fn a_decayed_lock_is_written_back_so_later_runs_see_it_cleared() {
        let dir = TempDir::new("lock-persist-decay");
        let mut provider = DecayingJsonCounterLockProvider::new(dir.path(), "l", 1, None);
        provider.load_lock().unwrap();
        provider.get_lock_mut().increment(1);
        provider.save_lock().unwrap();

        let mut decaying = DecayingJsonCounterLockProvider::new(
            dir.path(),
            "l",
            1,
            Some(chrono::Duration::seconds(-1)),
        );
        decaying.load_lock().unwrap();

        let mut fresh = DecayingJsonCounterLockProvider::new(dir.path(), "l", 1, None);
        fresh.load_lock().unwrap();
        assert!(
            !fresh.get_lock().is_locked(),
            "the cleared state should have been saved"
        );
    }

    #[test]
    fn the_configured_threshold_overrides_the_stored_one() {
        let dir = TempDir::new("lock-threshold");
        let mut provider = DecayingJsonCounterLockProvider::new(dir.path(), "l", 10, None);
        provider.load_lock().unwrap();
        provider.get_lock_mut().increment(3);
        provider.save_lock().unwrap();
        assert!(!provider.get_lock().is_locked(), "three of ten");

        let mut stricter = DecayingJsonCounterLockProvider::new(dir.path(), "l", 2, None);
        stricter.load_lock().unwrap();
        stricter.get_lock_mut().increment(0);

        assert_eq!(stricter.get_lock().threshold, 2);
    }
}
