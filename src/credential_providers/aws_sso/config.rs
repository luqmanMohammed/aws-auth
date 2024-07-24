use super::utils;
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug)]
pub enum AwsSsoConfigError {
    InvalidConfig(serde_json::Error),
    ConfigNotFound(PathBuf, std::io::Error),
}

#[derive(Debug, Deserialize)]
pub struct AwsSsoConfig {
    #[serde(alias = "startURL")]
    pub start_url: String,
    #[serde(alias = "ssoRegion")]
    pub sso_reigon: String,
    #[serde(alias = "maxRetries")]
    pub max_retries: Option<usize>,
    #[serde(alias = "retryInterval")]
    pub retry_interval: Option<Duration>,
    #[serde(alias = "expiresIn")]
    pub expires_in: Option<Duration>,
}

impl std::fmt::Display for AwsSsoConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AwsSsoConfigError::InvalidConfig(err) => writeln!(
                f,
                "Invalid config due to missing fields or Invalid Syntax: {}",
                err
            ),
            AwsSsoConfigError::ConfigNotFound(path, err) => {
                writeln!(f, "Config file not found at {:?}: {}", path, err)
            }
        }
    }
}

impl std::error::Error for AwsSsoConfigError {}

fn resolve_config_path(config_path: Option<&Path>) -> PathBuf {
    config_path.map_or_else(
        || {
            let home_dir = utils::resolve_awssso_home(None);
            home_dir.join("config.json")
        },
        PathBuf::from,
    )
}

impl AwsSsoConfig {
    fn load_config_from_reader<R: Read>(reader: R) -> Result<Self, AwsSsoConfigError> {
        serde_json::from_reader::<R, AwsSsoConfig>(reader).map_err(AwsSsoConfigError::InvalidConfig)
    }

    pub fn load_config(config_path: Option<&Path>) -> Result<Self, AwsSsoConfigError> {
        let config_path = resolve_config_path(config_path);
        let config_file = File::open(&config_path)
            .map_err(|err| AwsSsoConfigError::ConfigNotFound(config_path, err))?;
        AwsSsoConfig::load_config_from_reader(config_file)
    }
}
