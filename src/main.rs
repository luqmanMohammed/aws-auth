mod cache;
mod cmd;
mod credential_providers;
// mod eks;
// mod oidc;

mod types;

use aws_config::Region;
use cache::CacheManager;
use cmd::Arguments;
use credential_providers::{
    aws_sso::AwsSsoCredentialProvider, provide_credentials, ProvideCredentialsInput,
};

#[tokio::main]
async fn main() -> Result<(), ()> {
    // generate_eks_auth_token().await.map_err(|err| eprintln!("ERROR: {}", err))?;
    let args = Arguments::from_env_args().map_err(|err| eprintln!("ERROR: {}", err))?;

    let cache_manager = CacheManager::new(&args);
    let credential_provider = AwsSsoCredentialProvider::minimal(
        "https://fake.awsapps.com/start".to_string(),
        Region::new("eu-west-2"),
    );

    let exec_creds = match cache_manager.resolve_cache_hit() {
        Some(hit) => hit,
        None => {
            let creds =
                provide_credentials(credential_provider, &ProvideCredentialsInput::from(args))
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
