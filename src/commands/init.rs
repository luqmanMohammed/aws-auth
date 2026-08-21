use crate::aws_sso::config::AwsSsoConfig;
use crate::utils::private_fs;
use crate::utils::resolve_config_dir;
use std::fs::File;
use std::path::PathBuf;

// Directories relative to the config directory
const RELATIVE_DIRS: [&str; 1] = ["eks"];

pub struct ExecInitInputs {
    pub config_dir: Option<PathBuf>,
    pub update: bool,
    pub recreate: bool,

    pub sso_start_url: Option<String>,
    pub sso_region: Option<String>,
    pub max_attempts: Option<usize>,
    pub initial_delay: Option<std::time::Duration>,
    pub retry_interval: Option<std::time::Duration>,
    pub create_token_retry_threshold: Option<u64>,
    pub create_token_lock_decay: Option<chrono::Duration>,
    pub no_browser: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct InitConfig {
    #[serde(flatten)]
    sso_config: AwsSsoConfig,
}

pub fn exec_init(exec_inputs: ExecInitInputs) -> Result<(), std::io::Error> {
    let config_dir = resolve_config_dir(exec_inputs.config_dir.as_deref())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err))?;
    let config_dir_exists = config_dir.exists();
    let config_file = config_dir.join("config.json");

    if config_dir_exists && !(exec_inputs.recreate || exec_inputs.update) {
        eprintln!(
            "INFO: Config dir exists at {config_dir:?}. No update flags are provided. Assuming dry-run and exiting with success"
        );
        return Ok(());
    }

    if exec_inputs.update && exec_inputs.recreate {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cannot --update and --recreate at the same time.",
        ));
    }

    let sso_config = if exec_inputs.update && config_dir_exists {
        let mut sso_config = AwsSsoConfig::load_config(&config_file)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        if let Some(start_url) = exec_inputs.sso_start_url {
            sso_config.start_url = start_url;
        }
        if let Some(sso_region) = exec_inputs.sso_region {
            sso_config.sso_region = sso_region;
        }
        if let Some(max_attempts) = exec_inputs.max_attempts {
            sso_config.max_attempts = Some(max_attempts);
        }
        if let Some(initial_delay) = exec_inputs.initial_delay {
            sso_config.initial_delay = Some(initial_delay);
        }
        if let Some(retry_interval) = exec_inputs.retry_interval {
            sso_config.retry_interval = Some(retry_interval);
        }
        if let Some(create_token_retry_threshold) = exec_inputs.create_token_retry_threshold {
            sso_config.create_token_retry_threshold = Some(create_token_retry_threshold);
        }
        if let Some(create_token_lock_decay) = exec_inputs.create_token_lock_decay {
            sso_config.create_token_lock_decay = Some(create_token_lock_decay);
        }
        if let Some(no_browser) = exec_inputs.no_browser {
            sso_config.no_browser = Some(no_browser);
        }
        sso_config
    } else if let (Some(start_url), Some(sso_region)) =
        (exec_inputs.sso_start_url, exec_inputs.sso_region)
    {
        AwsSsoConfig {
            start_url,
            sso_region,
            max_attempts: exec_inputs.max_attempts,
            initial_delay: exec_inputs.initial_delay,
            retry_interval: exec_inputs.retry_interval,
            create_token_retry_threshold: exec_inputs.create_token_retry_threshold,
            create_token_lock_decay: exec_inputs.create_token_lock_decay,
            no_browser: exec_inputs.no_browser,
        }
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "--sso-start-url and --sso-region are required when not updating.",
        ))?
    };

    if !config_dir_exists || exec_inputs.recreate {
        if config_dir_exists && exec_inputs.recreate {
            eprintln!(
                "INFO: Recreating configuration directory at {}",
                config_dir.display()
            );
            std::fs::remove_dir_all(&config_dir)?;
        }
        private_fs::create_dir_all(&config_dir)?;
        for dir in RELATIVE_DIRS {
            private_fs::create_dir_all(&config_dir.join(dir))?;
        }
        eprintln!(
            "INFO: Successfully created configuration directory at {}",
            config_dir.display()
        );
    }

    let config_file = File::create(&config_file)?;
    serde_json::to_writer_pretty(config_file, &InitConfig { sso_config })
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    eprintln!(
        "INFO: Successfully initialized/updated configuration in {}",
        config_dir.display()
    );
    Ok(())
}

// Written by an AI assistant and not human reviewed.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;
    use std::path::{Path, PathBuf};

    fn inputs(config_dir: &Path) -> ExecInitInputs {
        ExecInitInputs {
            config_dir: Some(config_dir.to_path_buf()),
            update: false,
            recreate: false,
            sso_start_url: None,
            sso_region: None,
            max_attempts: None,
            initial_delay: None,
            retry_interval: None,
            create_token_retry_threshold: None,
            create_token_lock_decay: None,
            no_browser: None,
        }
    }

    /// Initialises into a directory that does not exist yet, which is the only case that
    /// writes a config without an update flag.
    fn created(dir: &TempDir, start_url: &str, region: &str) -> PathBuf {
        let config_dir = dir.join("cfg");
        let mut args = inputs(&config_dir);
        args.sso_start_url = Some(start_url.to_string());
        args.sso_region = Some(region.to_string());
        exec_init(args).expect("initial create should succeed");
        config_dir
    }

    fn config_at(config_dir: &Path) -> AwsSsoConfig {
        AwsSsoConfig::load_config(&config_dir.join("config.json"))
            .expect("config should be readable")
    }

    #[test]
    fn creates_the_config_and_the_relative_directories() {
        let dir = TempDir::new("init-create");

        let config_dir = created(&dir, "https://a.awsapps.com/start", "eu-west-2");

        assert!(config_dir.join("config.json").is_file());
        for relative in RELATIVE_DIRS {
            assert!(
                config_dir.join(relative).is_dir(),
                "{relative} should have been created"
            );
        }
        let config = config_at(&config_dir);
        assert_eq!(config.start_url, "https://a.awsapps.com/start");
        assert_eq!(config.sso_region, "eu-west-2");
    }

    #[test]
    fn both_the_start_url_and_the_region_are_required() {
        let dir = TempDir::new("init-required");

        let mut only_url = inputs(&dir.join("a"));
        only_url.sso_start_url = Some("https://a.awsapps.com/start".to_string());
        assert!(exec_init(only_url).is_err(), "a region is also needed");
        assert!(!dir.join("a").exists(), "nothing should have been created");

        let mut only_region = inputs(&dir.join("b"));
        only_region.sso_region = Some("eu-west-2".to_string());
        assert!(
            exec_init(only_region).is_err(),
            "a start url is also needed"
        );
        assert!(!dir.join("b").exists(), "nothing should have been created");
    }

    #[test]
    fn an_existing_config_is_left_alone_without_a_flag() {
        let dir = TempDir::new("init-dryrun");
        let config_dir = created(&dir, "https://a.awsapps.com/start", "eu-west-2");

        let mut args = inputs(&config_dir);
        args.sso_region = Some("us-east-1".to_string());
        exec_init(args).expect("a dry run reports success");

        assert_eq!(
            config_at(&config_dir).sso_region,
            "eu-west-2",
            "without --update or --recreate nothing changes"
        );
    }

    #[test]
    fn update_changes_only_the_values_given() {
        let dir = TempDir::new("init-update");
        let config_dir = created(&dir, "https://a.awsapps.com/start", "eu-west-2");

        let mut args = inputs(&config_dir);
        args.update = true;
        args.sso_region = Some("us-east-1".to_string());
        exec_init(args).expect("update should succeed");

        let config = config_at(&config_dir);
        assert_eq!(config.sso_region, "us-east-1", "the region was replaced");
        assert_eq!(
            config.start_url, "https://a.awsapps.com/start",
            "the untouched value survives"
        );
    }

    #[test]
    fn update_can_set_the_optional_values() {
        let dir = TempDir::new("init-update-opt");
        let config_dir = created(&dir, "https://a.awsapps.com/start", "eu-west-2");

        let mut args = inputs(&config_dir);
        args.update = true;
        args.max_attempts = Some(7);
        args.create_token_retry_threshold = Some(3);
        args.no_browser = Some(true);
        exec_init(args).expect("update should succeed");

        let config = config_at(&config_dir);
        assert_eq!(config.max_attempts, Some(7));
        assert_eq!(config.create_token_retry_threshold, Some(3));
        assert_eq!(config.no_browser, Some(true));
    }

    #[test]
    fn recreate_replaces_the_directory() {
        let dir = TempDir::new("init-recreate");
        let config_dir = created(&dir, "https://a.awsapps.com/start", "eu-west-2");
        let stale = config_dir.join("stale-file");
        std::fs::write(&stale, b"x").unwrap();

        let mut args = inputs(&config_dir);
        args.recreate = true;
        args.sso_start_url = Some("https://b.awsapps.com/start".to_string());
        args.sso_region = Some("ap-south-1".to_string());
        exec_init(args).expect("recreate should succeed");

        assert_eq!(
            config_at(&config_dir).start_url,
            "https://b.awsapps.com/start"
        );
        assert!(!stale.exists(), "the directory was replaced");
    }

    #[test]
    fn recreate_without_the_required_values_keeps_the_existing_config() {
        // Validation used to happen after the directory had already been removed.
        let dir = TempDir::new("init-recreate-bad");
        let config_dir = created(&dir, "https://a.awsapps.com/start", "eu-west-2");
        let precious = config_dir.join("cache.json");
        std::fs::write(&precious, b"credentials").unwrap();

        let mut args = inputs(&config_dir);
        args.recreate = true;
        assert!(exec_init(args).is_err(), "the region is missing");

        assert!(precious.exists(), "the cache must not have been destroyed");
        assert_eq!(
            config_at(&config_dir).start_url,
            "https://a.awsapps.com/start",
            "the config must still be intact"
        );
    }

    #[test]
    fn update_and_recreate_together_are_rejected() {
        let dir = TempDir::new("init-both");
        let config_dir = created(&dir, "https://a.awsapps.com/start", "eu-west-2");

        let mut args = inputs(&config_dir);
        args.update = true;
        args.recreate = true;
        args.sso_start_url = Some("https://b.awsapps.com/start".to_string());
        args.sso_region = Some("ap-south-1".to_string());

        assert!(exec_init(args).is_err(), "the two flags are exclusive");
    }

    #[test]
    fn update_on_a_directory_without_a_config_reports_the_missing_file() {
        let dir = TempDir::new("init-update-empty");
        std::fs::create_dir_all(dir.join("empty")).unwrap();

        let mut args = inputs(&dir.join("empty"));
        args.update = true;
        args.sso_region = Some("eu-west-2".to_string());

        assert!(exec_init(args).is_err(), "there is nothing to update");
    }
}
