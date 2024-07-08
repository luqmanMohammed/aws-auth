use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_EXEC_CREDENTIALS_KIND: &str = "";
pub const DEFAULT_EXEC_CREDENTIALS_API_VERSION: &str = "";

#[derive(Debug, Deserialize, Serialize)]
pub struct K8sExecCredentialsStatus {
    #[serde(alias = "expirationTimestamp")]
    pub expiration_timestamp: DateTime<Utc>,
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct K8sExecCredentials {
    pub kind: String,
    #[serde(alias = "apiVersion")]
    pub api_version: String,
    pub spec: HashMap<String, serde_json::Value>,
    pub status: K8sExecCredentialsStatus,
}
