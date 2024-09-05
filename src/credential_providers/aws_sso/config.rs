use crate::credential_providers::aws_sso::utils;
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug)]
pub enum Error {
    InvalidConfig(serde_json::Error),
    ConfigNotFound(PathBuf, std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
pub struct AwsSsoConfig {
    #[serde(alias = "startURL")]
    pub start_url: String,
    #[serde(alias = "ssoRegion")]
    pub sso_reigon: String,
    #[serde(alias = "maxAttempts")]
    pub max_attempts: Option<usize>,
    #[serde(alias = "initialDelay")]
    pub initial_delay: Option<Duration>,
    #[serde(alias = "retryInterval")]
    pub retry_interval: Option<Duration>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidConfig(err) => writeln!(
                f,
                "Invalid config due to missing fields or Invalid Syntax: {}",
                err
            ),
            Error::ConfigNotFound(path, err) => {
                writeln!(f, "Config file not found at {:?}: {}", path, err)
            }
        }
    }
}

impl std::error::Error for Error {}

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
    fn load_config_from_reader<R: Read>(reader: R) -> Result<Self> {
        serde_json::from_reader::<R, AwsSsoConfig>(reader).map_err(Error::InvalidConfig)
    }

    pub fn load_config(config_path: Option<&Path>) -> Result<Self> {
        let config_path = resolve_config_path(config_path);
        let config_file =
            File::open(&config_path).map_err(|err| Error::ConfigNotFound(config_path, err))?;
        AwsSsoConfig::load_config_from_reader(config_file)
    }
}
