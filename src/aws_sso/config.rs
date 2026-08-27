use chrono::TimeDelta;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULT_CREATE_TOKEN_LOCK_THRESHOLD: u64 = 5;
const DEFAULT_CREATE_TOKEN_LOCK_DECAY: TimeDelta = TimeDelta::seconds(2 * 3600);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid config due to missing fields or Invalid Syntax: {0}")]
    InvalidConfig(#[from] serde_json::Error),
    #[error("Config file not found at {:?}: {}. Run `aws-auth init --help` to get help initializing config", .0, .1)]
    ConfigNotFound(PathBuf, std::io::Error),
    #[error("Config field {0} is invalid: {1}")]
    InvalidField(&'static str, &'static str),
    #[error("Could not determine a home directory to hold the config; pass --config-dir")]
    HomeDirNotFound,
}

pub type Result<T> = std::result::Result<T, Error>;

/// The on-disk shape. Field names and types here define the config file format.
#[derive(Debug, Deserialize, Serialize)]
pub struct UnverifiedSsoConfig {
    #[serde(rename = "startURL")]
    pub start_url: String,
    #[serde(rename = "ssoRegion")]
    pub sso_region: String,
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
    pub create_token_lock_decay: Option<TimeDelta>,
    #[serde(rename = "noBrowser", skip_serializing_if = "Option::is_none")]
    pub no_browser: Option<bool>,
}

impl UnverifiedSsoConfig {
    pub fn new(start_url: String, sso_region: String) -> Self {
        UnverifiedSsoConfig {
            start_url,
            sso_region,
            max_attempts: None,
            initial_delay: None,
            retry_interval: None,
            create_token_retry_threshold: None,
            create_token_lock_decay: None,
            no_browser: None,
        }
    }

    fn from_reader<R: Read>(reader: R) -> Result<Self> {
        Ok(serde_json::from_reader(reader)?)
    }

    pub fn from_config_file(config_path: &Path) -> Result<Self> {
        let config_file = File::open(config_path)
            .map_err(|err| Error::ConfigNotFound(config_path.to_path_buf(), err))?;
        Self::from_reader(config_file)
    }

    pub fn verify(self) -> Result<AwsSsoConfig> {
        if self.start_url.trim().is_empty() {
            return Err(Error::InvalidField("startURL", "must not be empty"));
        }
        if self.sso_region.trim().is_empty() {
            return Err(Error::InvalidField("ssoRegion", "must not be empty"));
        }
        if self.max_attempts == Some(0) {
            return Err(Error::InvalidField("maxAttempts", "must be at least 1"));
        }
        // A negative decay puts every deadline in the past, so the lock would clear itself on
        // load and silently stop guarding anything. Zero is the way to ask for that.
        if self
            .create_token_lock_decay
            .is_some_and(|decay| decay < TimeDelta::zero())
        {
            return Err(Error::InvalidField(
                "createTokenLockDecay",
                "must not be negative; use 0 to disable decay",
            ));
        }
        Ok(AwsSsoConfig(self))
    }
}

/// Only reachable through [`UnverifiedSsoConfig::verify`], so `startURL` and `ssoRegion` are
/// non-empty here, `maxAttempts` is at least one when set, and `createTokenLockDecay` is never
/// negative.
///
/// The accessors returning a bare value resolve their default here. The three still returning an
/// `Option` are defaulted by `AuthManager::new` instead, which owns the polling constants it
/// applies whether or not a config supplied them.
#[derive(Debug, Serialize)]
pub struct AwsSsoConfig(UnverifiedSsoConfig);

impl AwsSsoConfig {
    pub fn start_url(&self) -> &str {
        &self.0.start_url
    }

    pub fn sso_region(&self) -> &str {
        &self.0.sso_region
    }

    pub fn max_attempts(&self) -> Option<usize> {
        self.0.max_attempts
    }

    pub fn initial_delay(&self) -> Option<Duration> {
        self.0.initial_delay
    }

    pub fn retry_interval(&self) -> Option<Duration> {
        self.0.retry_interval
    }

    pub fn create_token_retry_threshold(&self) -> u64 {
        self.0
            .create_token_retry_threshold
            .unwrap_or(DEFAULT_CREATE_TOKEN_LOCK_THRESHOLD)
    }

    /// `None` once the configured decay is zero, which disables decay entirely.
    pub fn create_token_lock_decay(&self) -> Option<TimeDelta> {
        match self.0.create_token_lock_decay {
            Some(decay) if decay.num_seconds() == 0 => None,
            Some(decay) => Some(decay),
            None => Some(DEFAULT_CREATE_TOKEN_LOCK_DECAY),
        }
    }

    pub fn no_browser(&self) -> bool {
        self.0.no_browser.unwrap_or(false)
    }
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;

    fn config(decay: Option<TimeDelta>) -> UnverifiedSsoConfig {
        let mut config = UnverifiedSsoConfig::new(
            "https://a.awsapps.com/start".to_string(),
            "eu-west-2".to_string(),
        );
        config.create_token_lock_decay = decay;
        config
    }

    #[test]
    fn a_negative_lock_decay_is_rejected() {
        let err = config(Some(TimeDelta::seconds(-1)))
            .verify()
            .expect_err("a negative decay can never be honoured");

        assert!(
            matches!(err, Error::InvalidField("createTokenLockDecay", _)),
            "got {err:?}"
        );
    }

    #[test]
    fn a_zero_lock_decay_is_accepted_and_disables_decay() {
        let config = config(Some(TimeDelta::zero()))
            .verify()
            .expect("zero is the documented way to disable decay");

        assert_eq!(config.create_token_lock_decay(), None);
    }

    #[test]
    fn an_absent_lock_decay_falls_back_to_the_default() {
        let config = config(None).verify().expect("an absent decay is valid");

        assert_eq!(
            config.create_token_lock_decay(),
            Some(DEFAULT_CREATE_TOKEN_LOCK_DECAY)
        );
    }
}
