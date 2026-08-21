use crate::utils::private_fs;
use aws_config::Region;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const CACHE_FILE_PREFIX: &str = "eks-";
const PRUNE_UNTOUCHED_FOR: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Deserialize)]
struct K8sExecCredential {
    status: K8sExecCredentialStatus,
}

#[derive(Debug, Deserialize)]
struct K8sExecCredentialStatus {
    #[serde(alias = "expirationTimestamp")]
    expiration_timestamp: DateTime<Utc>,
}

pub struct CacheManager {
    cache_dir: PathBuf,
    cache_path: PathBuf,
}

pub struct CacheManagerInputs<'a> {
    pub account_id: &'a str,
    pub role: &'a str,
    pub region: &'a Region,
    pub cluster: &'a str,
    pub cache_dir: &'a Path,
}

impl CacheManager {
    pub fn new(args: &CacheManagerInputs) -> Self {
        let cache_file_name = format!(
            "{CACHE_FILE_PREFIX}{account}-{role}-{region}-{cluster}",
            account = args.account_id,
            role = args.role,
            region = args.region,
            cluster = args.cluster
        );

        let mut cache_path = PathBuf::new();
        cache_path.push(args.cache_dir);
        cache_path.push(cache_file_name);

        Self {
            cache_dir: args.cache_dir.to_path_buf(),
            cache_path,
        }
    }

    pub fn resolve_cache_hit(&self) -> Option<String> {
        fs::read_to_string(&self.cache_path)
            .ok()
            .and_then(|content| {
                serde_json::from_str::<K8sExecCredential>(&content)
                    .ok()
                    .and_then(|k8s_exec_creds| {
                        if Utc::now() + Duration::seconds(30)
                            < k8s_exec_creds.status.expiration_timestamp
                        {
                            Some(content)
                        } else {
                            None
                        }
                    })
            })
    }

    pub fn cache_credentials(&self, creds: &str) -> Result<(), std::io::Error> {
        private_fs::create_dir_all(&self.cache_dir)?;
        private_fs::write_atomic(&self.cache_path, creds.as_bytes())?;
        self.prune_expired();
        Ok(())
    }

    /// Best effort: a token left behind by a cluster or role no longer in use would otherwise
    /// sit in the cache directory forever. Only long untouched files are considered, which keeps
    /// this away from any path another process is actively refreshing.
    fn prune_expired(&self) {
        let Ok(entries) = fs::read_dir(&self.cache_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Only files this type writes: skips the temp files another process may be part way
            // through renaming into place, and anything else that shares the directory.
            let is_token_file = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(CACHE_FILE_PREFIX));
            if !is_token_file || path == self.cache_path || !path.is_file() {
                continue;
            }
            let untouched = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > PRUNE_UNTOUCHED_FOR);
            if !untouched {
                continue;
            }
            // Still checked, because the longest token a caller can ask for lasts exactly as long
            // as the window above.
            let expired = fs::read_to_string(&path)
                .ok()
                .and_then(|content| serde_json::from_str::<K8sExecCredential>(&content).ok())
                .is_some_and(|creds| creds.status.expiration_timestamp <= Utc::now());
            if expired {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

// Written by an AI assistant and not human reviewed.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;

    fn token_json(expiry: &str) -> String {
        format!(
            r#"{{"kind":"ExecCredential","apiVersion":"client.authentication.k8s.io/v1beta1","spec":{{}},"status":{{"expirationTimestamp":"{expiry}","token":"k8s-aws-v1.abc"}}}}"#
        )
    }

    fn manager(cache_dir: &Path) -> CacheManager {
        CacheManager::new(&CacheManagerInputs {
            account_id: "111111111111",
            role: "Admin",
            region: &Region::new("eu-west-2"),
            cluster: "mycluster",
            cache_dir,
        })
    }

    /// `touch -t` is POSIX and takes a local-time stamp, so the timestamp is formatted here
    /// rather than shelling out to a `date` whose flags differ between BSD and GNU.
    fn set_age_days(path: &Path, days: i64) {
        let when = chrono::Local::now() - Duration::days(days);
        let status = std::process::Command::new("touch")
            .arg("-t")
            .arg(when.format("%Y%m%d%H%M.%S").to_string())
            .arg(path)
            .status()
            .expect("touch should be available");
        assert!(status.success(), "touch should have set the timestamp");
        let age = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .expect("mtime should be readable")
            .elapsed()
            .expect("mtime should be in the past");
        assert!(
            age.as_secs() > (days as u64 - 1) * 24 * 60 * 60,
            "the file should now look {days} days old"
        );
    }

    #[test]
    fn a_live_token_is_a_cache_hit() {
        let dir = TempDir::new("eks-hit");
        let manager = manager(dir.path());
        manager
            .cache_credentials(&token_json("2030-01-01T00:00:00Z"))
            .expect("write should succeed");

        assert!(manager.resolve_cache_hit().is_some());
    }

    #[test]
    fn an_expired_token_is_not_a_cache_hit() {
        let dir = TempDir::new("eks-expired");
        let manager = manager(dir.path());
        manager
            .cache_credentials(&token_json("2020-01-01T00:00:00Z"))
            .expect("write should succeed");

        assert!(manager.resolve_cache_hit().is_none());
    }

    #[test]
    fn a_token_expiring_within_the_grace_period_is_not_a_cache_hit() {
        let dir = TempDir::new("eks-grace");
        let manager = manager(dir.path());
        let soon = (Utc::now() + Duration::seconds(10)).to_rfc3339();
        manager
            .cache_credentials(&token_json(&soon))
            .expect("write should succeed");

        assert!(
            manager.resolve_cache_hit().is_none(),
            "a token about to expire should not be reused"
        );
    }

    #[test]
    fn a_missing_or_unparseable_file_is_not_a_cache_hit() {
        let dir = TempDir::new("eks-junk");
        let manager = manager(dir.path());
        assert!(manager.resolve_cache_hit().is_none(), "missing file");

        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(&manager.cache_path, b"not json").unwrap();
        assert!(manager.resolve_cache_hit().is_none(), "unparseable file");
    }

    #[test]
    fn different_clusters_use_different_files() {
        let dir = TempDir::new("eks-distinct");
        let a = CacheManager::new(&CacheManagerInputs {
            account_id: "111111111111",
            role: "Admin",
            region: &Region::new("eu-west-2"),
            cluster: "one",
            cache_dir: dir.path(),
        });
        let b = CacheManager::new(&CacheManagerInputs {
            account_id: "111111111111",
            role: "Admin",
            region: &Region::new("eu-west-2"),
            cluster: "two",
            cache_dir: dir.path(),
        });

        assert_ne!(a.cache_path, b.cache_path);
    }

    #[test]
    fn the_account_role_region_and_cluster_all_affect_the_file_name() {
        let dir = TempDir::new("eks-names");
        let base = manager(dir.path());
        for (account, role, region, cluster) in [
            ("222222222222", "Admin", "eu-west-2", "mycluster"),
            ("111111111111", "Other", "eu-west-2", "mycluster"),
            ("111111111111", "Admin", "us-east-1", "mycluster"),
            ("111111111111", "Admin", "eu-west-2", "other"),
        ] {
            let other = CacheManager::new(&CacheManagerInputs {
                account_id: account,
                role,
                region: &Region::new(region.to_string()),
                cluster,
                cache_dir: dir.path(),
            });
            assert_ne!(
                base.cache_path, other.cache_path,
                "{account}/{role}/{region}/{cluster} should not share a file"
            );
        }
    }

    #[test]
    fn pruning_removes_only_long_untouched_expired_tokens() {
        let dir = TempDir::new("eks-prune");
        let manager = manager(dir.path());
        std::fs::create_dir_all(dir.path()).unwrap();

        let expired_old = dir.join("eks-1-old-eu-west-2-a");
        let expired_recent = dir.join("eks-2-recent-eu-west-2-b");
        let live_old = dir.join("eks-3-live-eu-west-2-c");
        let foreign = dir.join("unrelated.txt");
        std::fs::write(&expired_old, token_json("2020-01-01T00:00:00Z")).unwrap();
        std::fs::write(&expired_recent, token_json("2020-01-01T00:00:00Z")).unwrap();
        std::fs::write(&live_old, token_json("2030-01-01T00:00:00Z")).unwrap();
        std::fs::write(&foreign, b"keep me").unwrap();
        set_age_days(&expired_old, 30);
        set_age_days(&live_old, 30);
        set_age_days(&foreign, 30);

        manager
            .cache_credentials(&token_json("2030-01-01T00:00:00Z"))
            .expect("write should succeed");

        assert!(!expired_old.exists(), "expired and untouched: pruned");
        assert!(
            expired_recent.exists(),
            "expired but recent: a live user may be refreshing it"
        );
        assert!(live_old.exists(), "still valid: kept regardless of age");
        assert!(foreign.exists(), "not one of ours: left alone");
        assert!(
            manager.cache_path.exists(),
            "the token just written survives"
        );
    }

    #[test]
    fn pruning_ignores_in_flight_temporary_files() {
        let dir = TempDir::new("eks-prune-temp");
        let manager = manager(dir.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let temp = dir.join(".eks-4-inflight-eu-west-2-d.999.tmp");
        std::fs::write(&temp, token_json("2020-01-01T00:00:00Z")).unwrap();
        set_age_days(&temp, 30);

        manager
            .cache_credentials(&token_json("2030-01-01T00:00:00Z"))
            .expect("write should succeed");

        assert!(
            temp.exists(),
            "another process may be part way through renaming this into place"
        );
    }
}
