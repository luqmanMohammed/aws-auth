use crate::utils::private_fs;
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULT_CREATE_TOKEN_LOCK_THRESHOLD: u64 = 5;
const DEFAULT_CREATE_TOKEN_LOCK_DECAY: SignedDuration = SignedDuration::from_hours(2);

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
    pub create_token_lock_decay: Option<SignedDuration>,
    #[serde(rename = "noBrowser", skip_serializing_if = "Option::is_none")]
    pub no_browser: Option<bool>,
}

impl UnverifiedSsoConfig {
    pub fn new(start_url: String, sso_region: String) -> Self {
        UnverifiedSsoConfig {
            start_url,
            sso_region,
            retry_interval: None,
            create_token_retry_threshold: None,
            create_token_lock_decay: None,
            no_browser: None,
        }
    }

    fn from_slice(contents: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(contents)?)
    }

    /// A config still in the pre-jiff format is rewritten in the current one on load.
    pub fn from_config_file(config_path: &Path) -> Result<Self> {
        let contents = std::fs::read(config_path)
            .map_err(|err| Error::ConfigNotFound(config_path.to_path_buf(), err))?;
        let err = match Self::from_slice(&contents) {
            Ok(config) => return Ok(config),
            Err(err) => err,
        };
        let Ok(legacy) = serde_json::from_slice::<LegacySsoConfig>(&contents) else {
            return Err(err);
        };
        let config = Self::from(legacy);
        match serde_json::to_vec_pretty(&config)
            .map_err(std::io::Error::from)
            .and_then(|contents| private_fs::write_atomic(config_path, &contents))
        {
            Ok(()) => eprintln!("INFO: Migrated {config_path:?} to the current config format"),
            Err(err) => eprintln!(
                "WARN: Could not migrate {config_path:?} to the current config format: {err}"
            ),
        }
        Ok(config)
    }

    pub fn verify(self) -> Result<AwsSsoConfig> {
        if self.start_url.trim().is_empty() {
            return Err(Error::InvalidField("startURL", "must not be empty"));
        }
        if self.sso_region.trim().is_empty() {
            return Err(Error::InvalidField("ssoRegion", "must not be empty"));
        }
        // A negative decay puts every deadline in the past, so the lock would clear itself on
        // load and silently stop guarding anything. Zero is the way to ask for that.
        if self
            .create_token_lock_decay
            .is_some_and(|decay| decay.is_negative())
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
/// non-empty here and `createTokenLockDecay` is never negative.
///
/// The accessors returning a bare value resolve their default here. `retryInterval` is defaulted
/// by `AuthManager::new` instead, which owns the polling constants it applies whether or not a
/// config supplied them.
#[derive(Debug, Serialize)]
pub struct AwsSsoConfig(UnverifiedSsoConfig);

impl AwsSsoConfig {
    pub fn start_url(&self) -> &str {
        &self.0.start_url
    }

    pub fn sso_region(&self) -> &str {
        &self.0.sso_region
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
    pub fn create_token_lock_decay(&self) -> Option<SignedDuration> {
        match self.0.create_token_lock_decay {
            Some(decay) if decay.as_secs() == 0 => None,
            Some(decay) => Some(decay),
            None => Some(DEFAULT_CREATE_TOKEN_LOCK_DECAY),
        }
    }

    pub fn no_browser(&self) -> bool {
        self.0.no_browser.unwrap_or(false)
    }
}

/// The format written before the move from chrono to jiff, which differs only in
/// `createTokenLockDecay` being chrono's `[secs, nanos]` with `nanos` in `0..1e9`.
#[derive(Deserialize)]
struct LegacySsoConfig {
    #[serde(
        rename = "createTokenLockDecay",
        default,
        deserialize_with = "secs_nanos"
    )]
    create_token_lock_decay: Option<SignedDuration>,
    #[serde(flatten)]
    config: UnverifiedSsoConfig,
}

impl From<LegacySsoConfig> for UnverifiedSsoConfig {
    fn from(legacy: LegacySsoConfig) -> Self {
        UnverifiedSsoConfig {
            create_token_lock_decay: legacy.create_token_lock_decay,
            ..legacy.config
        }
    }
}

fn secs_nanos<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<SignedDuration>, D::Error> {
    const NANOS_PER_SEC: i32 = 1_000_000_000;
    Option::<(i64, i32)>::deserialize(deserializer)?
        .map(|(secs, nanos)| {
            if (0..NANOS_PER_SEC).contains(&nanos) {
                Ok(SignedDuration::new(secs, nanos))
            } else {
                Err(serde::de::Error::custom(
                    "nanos must be within 0..1_000_000_000",
                ))
            }
        })
        .transpose()
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;

    fn config(decay: Option<SignedDuration>) -> UnverifiedSsoConfig {
        let mut config = UnverifiedSsoConfig::new(
            "https://a.awsapps.com/start".to_string(),
            "eu-west-2".to_string(),
        );
        config.create_token_lock_decay = decay;
        config
    }

    #[test]
    fn a_negative_lock_decay_is_rejected() {
        let err = config(Some(SignedDuration::from_secs(-1)))
            .verify()
            .expect_err("a negative decay can never be honoured");

        assert!(
            matches!(err, Error::InvalidField("createTokenLockDecay", _)),
            "got {err:?}"
        );
    }

    #[test]
    fn a_zero_lock_decay_is_accepted_and_disables_decay() {
        let config = config(Some(SignedDuration::ZERO))
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

    const START: &str = r#""startURL":"https://a.awsapps.com/start","ssoRegion":"eu-west-2""#;

    fn written(dir: &TempDir, json: &str) -> PathBuf {
        let path = dir.join("config.json");
        std::fs::write(&path, json).expect("config should be writable");
        path
    }

    fn on_disk(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(path).expect("config should be readable"))
            .expect("config should be json")
    }

    #[test]
    fn a_lock_decay_round_trips_as_an_iso_8601_duration() {
        let json = serde_json::to_value(config(Some(SignedDuration::from_mins(30))))
            .expect("config should serialize");

        assert_eq!(json["createTokenLockDecay"], "PT30M");
        assert_eq!(
            UnverifiedSsoConfig::from_slice(json.to_string().as_bytes())
                .expect("config should parse")
                .create_token_lock_decay,
            Some(SignedDuration::from_mins(30))
        );
    }

    #[test]
    fn a_current_config_is_loaded_without_being_rewritten() {
        let dir = TempDir::new("config-current");
        let json = format!(r#"{{{START},"createTokenLockDecay":"PT30M"}}"#);
        let path = written(&dir, &json);

        let config = UnverifiedSsoConfig::from_config_file(&path).expect("config should load");

        assert_eq!(
            config.create_token_lock_decay,
            Some(SignedDuration::from_mins(30))
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
    }

    #[test]
    fn a_legacy_config_is_loaded_and_rewritten_in_the_current_format() {
        let dir = TempDir::new("config-legacy");
        let path = written(
            &dir,
            &format!(
                r#"{{{START},"retryInterval":{{"secs":10,"nanos":0}},"createTokenRetryThreshold":2,"createTokenLockDecay":[1800,0],"noBrowser":true}}"#
            ),
        );

        let config = UnverifiedSsoConfig::from_config_file(&path).expect("config should load");

        assert_eq!(
            config.create_token_lock_decay,
            Some(SignedDuration::from_mins(30))
        );
        assert_eq!(
            on_disk(&path),
            serde_json::json!({
                "startURL": "https://a.awsapps.com/start",
                "ssoRegion": "eu-west-2",
                "retryInterval": { "secs": 10, "nanos": 0 },
                "createTokenRetryThreshold": 2,
                "createTokenLockDecay": "PT30M",
                "noBrowser": true,
            })
        );
    }

    #[test]
    fn a_legacy_negative_lock_decay_keeps_its_value() {
        let dir = TempDir::new("config-legacy-negative");
        let path = written(
            &dir,
            &format!(r#"{{{START},"createTokenLockDecay":[-2,500000000]}}"#),
        );

        let config = UnverifiedSsoConfig::from_config_file(&path).expect("config should load");

        assert_eq!(
            config.create_token_lock_decay,
            Some(SignedDuration::from_millis(-1500))
        );
    }

    #[test]
    fn a_config_invalid_in_both_formats_is_rejected_and_left_alone() {
        let dir = TempDir::new("config-invalid");
        let json = format!(r#"{{{START},"createTokenLockDecay":[0,1000000000]}}"#);
        let path = written(&dir, &json);

        let result = UnverifiedSsoConfig::from_config_file(&path);

        assert!(
            matches!(result, Err(Error::InvalidConfig(_))),
            "got {result:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
    }
}
