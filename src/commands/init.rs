use crate::aws_sso::config::AwsSsoConfig;
use crate::utils::resolve_config_dir;
use std::fs::File;
use std::path::PathBuf;

// Directories relative to the config directory
const RELATIVE_DIRS: [&str; 2] = ["eks", "bin"];

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
}

#[derive(Debug, serde::Serialize)]
struct InitConfig {
    #[serde(flatten)]
    sso_config: AwsSsoConfig,
}

pub fn exec_init(exec_inputs: ExecInitInputs) -> Result<(), std::io::Error> {
    let config_dir = resolve_config_dir(exec_inputs.config_dir.as_deref());
    let config_dir_exists = config_dir.exists();
    let config_file = config_dir.join("config.json");

    if config_dir_exists && !(exec_inputs.recreate && exec_inputs.update) {
        println!("INFO: Config dir exists at {config_dir:?}. No update flags are provided. Assuming dry-run and exiting with success");
        return Ok(());
    }

    if exec_inputs.update && exec_inputs.recreate {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cannot --update and --recreate at the same time.",
        ));
    }

    if !config_dir_exists || exec_inputs.recreate {
        if config_dir_exists && exec_inputs.recreate {
            println!(
                "INFO: Recreating configuration directory at {}",
                config_dir.display()
            );
            std::fs::remove_dir_all(&config_dir)?;
        }
        std::fs::create_dir_all(&config_dir)?;
        for dir in RELATIVE_DIRS {
            std::fs::create_dir_all(config_dir.join(dir))?;
        }
        println!(
            "INFO: Successfully created configuration directory at {}",
            config_dir.display()
        );
    }

    let sso_config = if exec_inputs.update && config_dir_exists {
        let mut sso_config = AwsSsoConfig::load_config(&config_file)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        if let Some(start_url) = exec_inputs.sso_start_url {
            sso_config.start_url = start_url;
        }
        if let Some(sso_region) = exec_inputs.sso_region {
            sso_config.sso_reigon = sso_region;
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
        sso_config
    } else if exec_inputs.sso_start_url.is_some() || exec_inputs.sso_region.is_some() {
        AwsSsoConfig {
            start_url: exec_inputs.sso_start_url.unwrap(),
            sso_reigon: exec_inputs.sso_region.unwrap(),
            max_attempts: exec_inputs.max_attempts,
            initial_delay: exec_inputs.initial_delay,
            retry_interval: exec_inputs.retry_interval,
            create_token_retry_threshold: exec_inputs.create_token_retry_threshold,
            create_token_lock_decay: exec_inputs.create_token_lock_decay,
        }
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "--sso-start-url and --sso-region are required when not updating.",
        ))?
    };

    let config_file = File::create(&config_file)?;
    serde_json::to_writer_pretty(config_file, &InitConfig { sso_config })
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    println!(
        "INFO: Successfully initialized/updated configuration in {}",
        config_dir.display()
    );
    Ok(())
}
