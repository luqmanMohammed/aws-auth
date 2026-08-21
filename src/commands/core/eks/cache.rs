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
