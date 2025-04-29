use crate::aws_sso::{build_aws_sso_manager_with_cache_handling, AwsSsoManagerError};
use crate::cmd::Sso;
use crate::utils::{
    formatters::{json::JsonFormatter, text::TextFormatter, TabularFormatter},
    resolve_config_dir,
};

#[derive(Debug)]
pub enum Error {
    AwsSsoManager(AwsSsoManagerError),
    JsonFormatter(serde_json::Error),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::AwsSsoManager(error) => write!(f, "Error loading SSO accounts: {error}"),
            Error::JsonFormatter(error) => {
                write!(
                    f,
                    "Error formatting SSO accounts using json output: {error}"
                )
            }
        }
    }
}

pub async fn exec_sso(subcommand: Sso) -> Result<(), Error> {
    match subcommand {
        Sso::ListAccounts { common, formatting } => {
            let config_dir = resolve_config_dir(common.config_dir.as_deref());
            let mut sso_manager = build_aws_sso_manager_with_cache_handling(
                &config_dir,
                common.sso_cache_dir.as_deref(),
            );

            let accounts = sso_manager
                .list_accounts(common.ignore_cache)
                .await
                .map_err(Error::AwsSsoManager)?;

            let omit_fields = formatting.omit_fields.iter().map(|v| v.as_str()).collect();
            let accounts = accounts
                .iter()
                .map(|account| {
                    [
                        account.account_id().unwrap(),
                        account.account_name().unwrap(),
                        account.email_address().unwrap(),
                    ]
                })
                .collect::<Vec<_>>();

            match formatting.output {
                crate::cmd::OutputFormat::Json => {
                    let formatter = JsonFormatter::new(omit_fields, formatting.no_headers);
                    let output = formatter
                        .format(&["accountId", "accountName", "accountEmail"], accounts)
                        .map_err(Error::JsonFormatter)?;
                    println!("{}", output)
                }
                crate::cmd::OutputFormat::Text => {
                    let formatter = TextFormatter::new(omit_fields, formatting.no_headers, " | ");
                    let output = formatter
                        .format(&["Account Id", "Account Name", "Account Email"], accounts)
                        .expect("TextFormatter should not fail");
                    println!("{}", output)
                }
            }
            Ok(())
        }
        Sso::ListAccountRoles {
            common,
            account,
            formatting,
        } => {
            let config_dir = resolve_config_dir(common.config_dir.as_deref());
            let mut sso_manager = build_aws_sso_manager_with_cache_handling(
                &config_dir,
                common.sso_cache_dir.as_deref(),
            );

            let roles = sso_manager
                .list_account_roles(&account, common.ignore_cache)
                .await
                .map_err(Error::AwsSsoManager)?;

            let omit_fields = formatting.omit_fields.iter().map(|v| v.as_str()).collect();
            let roles = roles
                .iter()
                .map(|role| [role.account_id().unwrap(), role.role_name().unwrap()])
                .collect::<Vec<_>>();

            match formatting.output {
                crate::cmd::OutputFormat::Json => {
                    let formatter = JsonFormatter::new(omit_fields, formatting.no_headers);
                    let output = formatter
                        .format(&["accountId", "roleName"], roles)
                        .map_err(Error::JsonFormatter)?;
                    println!("{}", output)
                }
                crate::cmd::OutputFormat::Text => {
                    let formatter = TextFormatter::new(omit_fields, formatting.no_headers, " | ");
                    let output = formatter
                        .format(&["Account Id", "Role Name"], roles)
                        .expect("TextFormatter should not fail");
                    println!("{}", output)
                }
            }
            Ok(())
        }
    }
}
