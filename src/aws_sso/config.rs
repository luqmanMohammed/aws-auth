use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid config due to missing fields or Invalid Syntax: {0}")]
    InvalidConfig(#[from] serde_json::Error),
    #[error("Config file not found at {:?}: {}. Run `aws-auth init --help` to get help initializing config", .0, .1)]
    ConfigNotFound(PathBuf, std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize, Serialize)]
pub struct AwsSsoConfig {
    #[serde(rename = "startURL")]
    pub start_url: String,
    #[serde(rename = "ssoRegion")]
    pub sso_reigon: String,
    #[serde(rename = "maxAttempts", skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<usize>,
    #[serde(rename = "initialDelay", skip_serializing_if = "Option::is_none")]
    pub initial_delay: Option<Duration>,
    #[serde(rename = "retryInterval", skip_serializing_if = "Option::is_none")]
    pub retry_interval: Option<Duration>,
    #[serde(
        rename = "createTokenRetryThreshold",
        skip_serializing_if = "Option::is_none"
    )]
    pub create_token_retry_threshold: Option<u64>,
    #[serde(
        rename = "createTokenLockDecay",
        skip_serializing_if = "Option::is_none"
    )]
    pub create_token_lock_decay: Option<chrono::Duration>,
}

impl AwsSsoConfig {
    fn load_config_from_reader<R: Read>(reader: R) -> Result<Self> {
        Ok(serde_json::from_reader::<R, AwsSsoConfig>(reader)?)
    }

    pub fn load_config(config_path: &Path) -> Result<Self> {
        let config_file = File::open(config_path)
            .map_err(|err| Error::ConfigNotFound(config_path.to_path_buf(), err))?;
        AwsSsoConfig::load_config_from_reader(config_file)
    }
}
