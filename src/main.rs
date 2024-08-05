#![warn(unused_extern_crates)]

mod cache;
mod cmd;
mod credential_providers;
mod types;
mod utils;

use cache::CacheManager;
use cmd::Arguments;
use credential_providers::{
    aws_sso::{config::AwsSsoConfig, AwsSsoCredentialProvider},
    provide_credentials, ProvideCredentialsInput,
};
use std::error::Error;

fn error_to_string(error: impl Error) -> String {
    error.to_string()
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = Arguments::from_env_args().map_err(error_to_string)?;
    let cache_manager = CacheManager::new(&args);

    let credential_provider: AwsSsoCredentialProvider = AwsSsoConfig::load_config(None)
        .map_err(error_to_string)?
        .into();

    let exec_creds = if let Some(hit) = cache_manager.resolve_cache_hit() {
        hit
    } else {
        let creds = provide_credentials(credential_provider, &ProvideCredentialsInput::from(args))
            .await
            .map_err(error_to_string)?;

        let string_creds = serde_json::to_string(&creds).map_err(error_to_string)?;
        cache_manager
            .cache_credentials(&string_creds)
            .map_err(error_to_string)?;
        string_creds
    };

    println!("{}", exec_creds);
    Ok(())
}
