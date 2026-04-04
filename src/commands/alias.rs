use crate::alias_providers::{build_alias_provider_and_load, AliasProviderError, ProvideAliases};
use crate::cmd::Alias;
use crate::utils::formatters::text::TextFormatter;
use crate::utils::formatters::TabularFormatter;
use crate::utils::{self, formatters::json::JsonFormatter};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error loading aliases: {0}")]
    AliasProvider(#[from] AliasProviderError),
    #[error("Alias {0} already exists, set overwrite flag to overwrite existing alias")]
    AliasAlreadyExists(String),
    #[error("Error formating aliases list using json output: {0}")]
    JsonFormatter(#[from] serde_json::Error),
}

pub fn exec_alias(subcommand: Alias) -> Result<(), Error> {
    match subcommand {
        Alias::Set {
            common,
            alias,
            account,
            role,
            overwrite,
        } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let mut alias_provider = build_alias_provider_and_load(&config_dir)?;
            if alias_provider.get_alias(&alias)?.is_some() && !overwrite {
                return Err(Error::AliasAlreadyExists(alias));
            }
            alias_provider
                .set_alias(&alias, &account, &role)
                .map_err(Error::AliasProvider)?;
        }
        Alias::Unset { common, alias } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let mut alias_provider = build_alias_provider_and_load(&config_dir)?;
            alias_provider.unset_alias(&alias)?;
        }
        Alias::List { common, formatting } => {
            let config_dir = utils::resolve_config_dir(common.config_dir.as_deref());
            let alias_provider = build_alias_provider_and_load(&config_dir)?;
            let aliases: Vec<[&str; 3]> = alias_provider.list_aliases()?;
            let omit_fields = formatting.omit_fields.iter().map(|v| v.as_str()).collect();

            match formatting.output {
                crate::cmd::OutputFormat::Json => {
                    let formatter = JsonFormatter::new(omit_fields, formatting.no_headers);
                    let output = formatter.format(&["alias", "accountId", "role"], aliases)?;
                    println!("{}", output)
                }
                crate::cmd::OutputFormat::Text => {
                    let formatter = TextFormatter::new(omit_fields, formatting.no_headers, " | ");
                    let output = formatter
                        .format(&["Alias", "Account Id", "Role"], aliases)
                        .expect("TextFormatter doesnt error. Returns result to satisfy trait");
                    println!("{}", output)
                }
            }
        }
    }
    Ok(())
}
