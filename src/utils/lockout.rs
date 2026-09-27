use crate::utils::expiring::{Expiring, ExpiringFile};
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use std::io;
use std::num::NonZeroU64;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
enum Counter {
    Counting(u64),
    Locked,
}

pub struct Lockout {
    file: ExpiringFile<Counter>,
    threshold: NonZeroU64,
    decay: Option<SignedDuration>,
}

impl Lockout {
    pub fn new(
        base_dir: &Path,
        name: &str,
        threshold: NonZeroU64,
        decay: Option<SignedDuration>,
    ) -> Self {
        Self {
            file: ExpiringFile::new(base_dir.join(name).with_extension("json")),
            threshold,
            decay,
        }
    }

    pub fn load(&mut self) -> io::Result<LoadedLockout<'_>> {
        Ok(LoadedLockout {
            counter: self.file.load()?,
            lockout: self,
        })
    }
}

pub struct LoadedLockout<'a> {
    lockout: &'a Lockout,
    counter: Option<Expiring<Counter>>,
}

impl LoadedLockout<'_> {
    pub fn is_locked(&self) -> bool {
        matches!(
            self.counter.as_ref().map(Expiring::data),
            Some(Counter::Locked)
        )
    }

    pub fn increment(&mut self) {
        let failures = match self.counter.as_ref().map(Expiring::data) {
            None => 1,
            Some(Counter::Counting(failures)) => failures.saturating_add(1),
            Some(Counter::Locked) => return,
        };
        self.counter = if failures >= self.lockout.threshold.get() {
            match self.lockout.decay {
                Some(decay) => Expiring::new(Counter::Locked, decay),
                None => Expiring::until(Counter::Locked, None),
            }
        } else {
            Expiring::until(Counter::Counting(failures), None)
        };
    }

    pub fn reset(&mut self) {
        self.counter = None;
    }

    pub fn save(self) -> io::Result<()> {
        match &self.counter {
            Some(counter) => self.lockout.file.store(counter),
            None => self.lockout.file.clear(),
        }
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;
    use jiff::Timestamp;

    fn lockout(dir: &TempDir, threshold: u64, decay: Option<SignedDuration>) -> Lockout {
        Lockout::new(
            dir.path(),
            "l",
            NonZeroU64::new(threshold).expect("non-zero threshold"),
            decay,
        )
    }

    fn fail(lockout: &mut Lockout) {
        let mut loaded = lockout.load().unwrap();
        loaded.increment();
        loaded.save().unwrap();
    }

    fn is_locked(lockout: &mut Lockout) -> bool {
        lockout.load().unwrap().is_locked()
    }

    fn deadline(dir: &TempDir) -> Option<Timestamp> {
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("l.json")).unwrap()).unwrap();
        json["expires_at"].as_str().map(|at| at.parse().unwrap())
    }

    #[test]
    fn a_missing_file_loads_clear() {
        let dir = TempDir::new("lockout-missing");
        assert!(!is_locked(&mut lockout(&dir, 1, None)));
    }

    #[test]
    fn failures_counted_across_runs_lock_at_the_threshold() {
        let dir = TempDir::new("lockout-threshold");

        fail(&mut lockout(&dir, 3, None));
        assert!(!is_locked(&mut lockout(&dir, 3, None)), "one of three");
        fail(&mut lockout(&dir, 3, None));
        assert!(!is_locked(&mut lockout(&dir, 3, None)), "two of three");
        fail(&mut lockout(&dir, 3, None));
        assert!(is_locked(&mut lockout(&dir, 3, None)), "three of three");
    }

    #[test]
    fn a_tripped_lockout_holds_until_the_decay_has_passed() {
        let dir = TempDir::new("lockout-decay");
        let decay = SignedDuration::from_hours(2);
        let before = Timestamp::now();

        fail(&mut lockout(&dir, 1, Some(decay)));

        let deadline = deadline(&dir).expect("a decaying lockout has a deadline");
        assert!(before + decay <= deadline && deadline <= Timestamp::now() + decay);
        assert!(is_locked(&mut lockout(&dir, 1, Some(decay))));
    }

    #[test]
    fn without_decay_a_tripped_lockout_never_expires() {
        let dir = TempDir::new("lockout-permanent");
        fail(&mut lockout(&dir, 1, None));

        assert_eq!(deadline(&dir), None);
        assert!(is_locked(&mut lockout(&dir, 1, None)));
    }

    #[test]
    fn a_lockout_past_its_deadline_loads_clear() {
        let dir = TempDir::new("lockout-expired");
        let past = Timestamp::now() - SignedDuration::from_secs(1);
        std::fs::write(
            dir.join("l.json"),
            format!(r#"{{"data":"Locked","expires_at":"{past}"}}"#),
        )
        .unwrap();

        assert!(!is_locked(&mut lockout(&dir, 1, None)));
    }

    #[test]
    fn further_failures_do_not_extend_a_tripped_lockout() {
        let dir = TempDir::new("lockout-no-extend");
        let decay = SignedDuration::from_hours(2);
        fail(&mut lockout(&dir, 1, Some(decay)));
        let first = deadline(&dir);

        fail(&mut lockout(&dir, 1, Some(decay)));

        assert_eq!(deadline(&dir), first);
    }

    #[test]
    fn an_old_format_or_corrupt_file_loads_clear_and_is_replaced_on_save() {
        for contents in [
            r#"{"threshold":1,"count":1,"locked_at":"2026-09-27T10:00:00Z"}"#,
            "not json",
        ] {
            let dir = TempDir::new("lockout-unreadable");
            std::fs::write(dir.join("l.json"), contents).unwrap();
            let mut lockout = lockout(&dir, 1, None);

            let mut loaded = lockout
                .load()
                .expect("an unreadable lockout is not an error");
            assert!(!loaded.is_locked(), "{contents}");
            loaded.increment();
            loaded.save().unwrap();

            assert!(is_locked(&mut lockout), "{contents}");
        }
    }

    #[test]
    fn resetting_and_saving_removes_the_file() {
        let dir = TempDir::new("lockout-reset");
        let mut lockout = lockout(&dir, 1, None);
        fail(&mut lockout);

        let mut loaded = lockout.load().unwrap();
        loaded.reset();
        loaded.save().unwrap();

        assert!(!dir.join("l.json").exists());
        assert!(!is_locked(&mut lockout));
    }

    #[test]
    fn raising_the_threshold_does_not_release_a_lockout() {
        let dir = TempDir::new("lockout-raised");
        fail(&mut lockout(&dir, 1, None));

        assert!(is_locked(&mut lockout(&dir, 10, None)));
    }
}
