use crate::cmd::Args;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

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
    cache_path: PathBuf,
}

impl CacheManager {
    pub fn new(args: &Args) -> CacheManager {
        let cache_file_name = format!(
            "eks-{account}-{role}-{region}-{cluster}",
            account = &args.account,
            role = &args.role,
            region = &args.region,
            cluster = &args.cluster_name
        );

        let mut cache_path = PathBuf::new();
        cache_path.push(&args.cache_dir);
        cache_path.push(cache_file_name);

        CacheManager { cache_path }
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
        fs::write(&self.cache_path, creds)
    }
}
