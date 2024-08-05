use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_EXEC_CREDENTIALS_KIND: &str = "ExecCredential";
pub const DEFAULT_EXEC_CREDENTIALS_API_VERSION: &str = "client.authentication.k8s.io/v1beta1";

#[derive(Debug, Deserialize, Serialize)]
pub struct K8sExecCredentialsStatus {
    #[serde(rename = "expirationTimestamp")]
    pub expiration_timestamp: DateTime<Utc>,
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct K8sExecCredentials {
    pub kind: String,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub spec: HashMap<String, serde_json::Value>,
    pub status: K8sExecCredentialsStatus,
}
