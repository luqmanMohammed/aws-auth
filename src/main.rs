mod cache;
mod cmd;
mod credential_providers;
// mod eks;
// mod oidc;

mod types;

use cache::CacheManager;
use cmd::Arguments;
use credential_providers::{
    aws_cmd::AwsCmdCredentialProvider, provide_credentials, ProvideCredentialsInput,
};

#[tokio::main]
async fn main() -> Result<(), ()> {
    // generate_eks_auth_token().await.map_err(|err| eprintln!("ERROR: {}", err))?;
    let args = Arguments::from_env_args().map_err(|err| eprintln!("ERROR: {}", err))?;

    let cache_manager = CacheManager::new(&args);

    let exec_creds = match cache_manager.resolve_cache_hit() {
        Some(hit) => hit,
        None => {
            let creds = provide_credentials(
                AwsCmdCredentialProvider {},
                &ProvideCredentialsInput::from(args),
            )
            .await
            .map_err(|err| eprintln!("ERROR: {}", err))?;

            let string_creds =
                serde_json::to_string(&creds).map_err(|err| eprintln!("ERROR: {}", err))?;
            cache_manager
                .cache_credentials(string_creds.as_str())
                .map_err(|err| eprintln!("ERROR: {}", err))?;
            string_creds
        }
    };
    println!("{}", exec_creds);

    Ok(())
}
