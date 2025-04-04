use crate::alias_providers::{build_alias_provider_and_load, AliasProviderError, ProvideAliases};
use crate::cmd::Alias;
use crate::utils;

pub fn exec_alias(subcommand: Alias) -> Result<(), AliasProviderError> {
    match subcommand {
        Alias::Set {
            common,
            alias,
            account,
            role,
        } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let mut alias_provider = build_alias_provider_and_load(&config_dir)?;
            alias_provider.set_alias(&alias, &account, &role)?;
        }
        Alias::Unset { common, alias } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let mut alias_provider = build_alias_provider_and_load(&config_dir)?;
            alias_provider.unset_alias(&alias)?;
        }
        Alias::List { common } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let alias_provider = build_alias_provider_and_load(&config_dir)?;
            let aliases = alias_provider.list_aliases()?;
            println!("\x1b[1m{:<25}\t{:<12}\tRole\x1b[0m", "Alias", "Account Id");
            for (alias, account, role) in aliases.iter() {
                println!("{:<25}\t{}\t{}", alias, account, role);
            }
        }
    }
    Ok(())
}
