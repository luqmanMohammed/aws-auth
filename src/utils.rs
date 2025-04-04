use crate::alias_providers::ProvideAliases;
use crate::cmd::{AssumeInput, CommonArgs};
use crate::common::AssumeIdentifier;
use std::env;
use std::path::{Path, PathBuf};

pub fn resolve_config_dir(config_dir: Option<&Path>) -> PathBuf {
    config_dir.map_or_else(
        || {
            let home_dir = home::home_dir().unwrap_or_else(env::temp_dir);
            home_dir.join(".aws-auth")
        },
        PathBuf::from,
    )
}

pub fn resolve_assume_identifier<'c, 'p: 'c, A: ProvideAliases>(
    provider: &'p mut A,
    common: &'c CommonArgs,
) -> Result<AssumeIdentifier<'c>, A::Error> {
    match &common.assume_input {
        AssumeInput {
            account: Some(a),
            role: Some(r),
            alias: None,
        } => Ok(AssumeIdentifier {
            account: a,
            role: r,
        }),
        AssumeInput {
            account: None,
            role: None,
            alias: Some(l),
        } => {
            provider.load_aliases()?;
            provider.get_alias(l)
        }
        _ => unreachable!("Clap should prevent code from reaching this branch"),
    }
}
