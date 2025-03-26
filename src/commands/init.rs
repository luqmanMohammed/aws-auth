use crate::credential_providers::aws_sso::config::AwsSsoConfig;
use crate::utils::resolve_config_dir;
use std::path::PathBuf;

// Directories relative to the config directory
const RELATIVE_DIRS: [&str; 1] = ["eks"];

pub struct ExecInitInputs {
    pub config_dir: Option<PathBuf>,
    pub recreate: bool,

    pub sso_start_url: String,
    pub sso_region: String,
    pub max_attempts: Option<usize>,
    pub initial_delay: Option<std::time::Duration>,
    pub retry_interval: Option<std::time::Duration>,
}

#[derive(Debug, serde::Serialize)]
struct InitConfig {
    #[serde(flatten)]
    sso_config: AwsSsoConfig,
}

pub fn exec_init(exec_inputs: ExecInitInputs) -> Result<(), std::io::Error> {
    let config_dir = resolve_config_dir(exec_inputs.config_dir.as_deref());
    if exec_inputs.recreate && config_dir.exists() {
        println!(
            "INFO: Removing existing config directory at {:?}",
            config_dir
        );
        std::fs::remove_dir_all(&config_dir)?;
    } else if config_dir.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "Config directory already exists at {:?}. Use the `--recreate` flag to overwrite.",
                config_dir
            ),
        ));
    }
    println!("INFO: Creating config directory at {:?}", config_dir);
    std::fs::create_dir_all(&config_dir)?;
    for path in RELATIVE_DIRS.iter() {
        let mut full_path = config_dir.clone();
        full_path.push(path);
        println!("INFO: Creating sub-directory at {:?}", full_path);
        std::fs::create_dir_all(&full_path)?;
    }
    let sso_config = AwsSsoConfig {
        start_url: exec_inputs.sso_start_url,
        sso_reigon: exec_inputs.sso_region,
        max_attempts: exec_inputs.max_attempts,
        initial_delay: exec_inputs.initial_delay,
        retry_interval: exec_inputs.retry_interval,
    };
    let config = InitConfig { sso_config };
    let config_path = config_dir.join("config.json");
    let file = std::fs::File::create(&config_path)?;
    serde_json::to_writer_pretty(&file, &config)?;
    println!("INFO: Config file created at {:?}", config_path);
    Ok(())
}
