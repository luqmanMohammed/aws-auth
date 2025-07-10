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
            if let Some((ldd, la)) = self.lock_decay_duration.zip(lock.locked_at) {
                if Utc::now() >= la + ldd {
                    lock = CounterLock {
                        threshold: self.threshold,
                        count: 0,
                        locked_at: None,
                    };
                    save_lock = true;
                }
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
