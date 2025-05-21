use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct CounterLock {
    threshold: u64,
    count: u64,
    is_locked: bool,
}

impl CounterLock {
    pub fn is_locked(&self) -> bool {
        self.is_locked
    }
    pub fn increment(&mut self, count: u64) {
        self.count += count;
        if self.count >= self.threshold {
            self.is_locked = true;
        }
    }
    pub fn reset(&mut self) {
        self.count = 0;
        self.is_locked = false;
    }
}

pub trait CounterLockProvider {
    type Error: std::error::Error;
    fn load_lock(&mut self) -> Result<(), Self::Error>;
    fn save_lock(&self) -> Result<(), Self::Error>;
    fn get_lock(&self) -> &CounterLock;
    fn get_lock_mut(&mut self) -> &mut CounterLock;
}

pub struct JsonCounterLockProvider {
    lock_path: PathBuf,
    lock: Option<CounterLock>,
    threshold: u64,
}

impl JsonCounterLockProvider {
    pub fn new(base_dir: &Path, lockname: &str, threshold: u64) -> Self {
        Self {
            lock_path: base_dir.join(lockname).with_extension("json"),
            lock: None,
            threshold,
        }
    }
}

impl CounterLockProvider for JsonCounterLockProvider {
    type Error = std::io::Error;

    fn load_lock(&mut self) -> Result<(), Self::Error> {
        let lock_path = &self.lock_path;
        if lock_path.exists() {
            let file = std::fs::File::open(lock_path)?;
            let mut lock: CounterLock = serde_json::from_reader(file)?;
            lock.threshold = self.threshold;
            self.lock = Some(lock);
        } else {
            self.lock = Some(CounterLock {
                threshold: self.threshold,
                count: 0,
                is_locked: false,
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
        self.lock.as_ref().unwrap()
    }

    fn get_lock_mut(&mut self) -> &mut CounterLock {
        self.lock.as_mut().unwrap()
    }
}
