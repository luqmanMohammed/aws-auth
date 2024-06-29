mod cache;
mod cmd;
mod creds;

use cache::CacheManager;
use cmd::Args;
use creds::{resolve_exec_credentials, OidcCmdResolver};

fn main() -> Result<(), ()> {
    let args = Args::from_env_args().map_err(|err| eprintln!("ERROR: {}", err))?;
    let cache_manager = CacheManager::new(&args);

    let exec_creds = match cache_manager.resolve_cache_hit() {
        Some(hit) => hit,
        None => {
            let creds = resolve_exec_credentials(OidcCmdResolver {}, &args)
                .map_err(|err| eprintln!("ERROR: {}", err))?;
            cache_manager
                .cache_credentials(&creds)
                .map_err(|err| eprintln!("ERROR: {}", err))?;
            creds
        }
    };
    println!("{}", exec_creds);

    Ok(())
}
