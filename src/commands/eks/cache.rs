use aws_config::Region;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

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
            "eks-{account}-{role}-{region}-{cluster}",
            account = &args.account_id,
            role = &args.role,
            region = &args.region,
            cluster = &args.cluster
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
        fs::create_dir_all(&self.cache_dir)?;
        fs::write(&self.cache_path, creds)
    }
}
