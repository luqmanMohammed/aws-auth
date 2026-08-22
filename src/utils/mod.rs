pub mod elog;
pub mod formatters;
pub mod lock;
pub mod private_fs;
#[cfg(test)]
pub mod test_support;
pub mod worker;

use crate::alias_providers::ProvideAliases;
use crate::aws_sso::ConfigError;
use crate::cmd::{AssumeInput, CommonArgs};
use crate::common::AssumeIdentifier;
use std::path::{Path, PathBuf};

pub fn resolve_config_dir(config_dir: Option<&Path>) -> Result<PathBuf, ConfigError> {
    match config_dir {
        Some(dir) => Ok(dir.to_path_buf()),
        // Falling back to a temp dir would put credentials somewhere world-writable, so an
        // unlocatable home is an error rather than a guess.
        None => Ok(home::home_dir()
            .ok_or(ConfigError::HomeDirNotFound)?
            .join(".aws-auth")),
    }
}

#[derive(Debug)]
pub enum AssumeIdResolverError<'a, PE: std::error::Error> {
    ProviderError(PE),
    AliasNotFoundError(&'a str),
}

impl<PE: std::error::Error> std::fmt::Display for AssumeIdResolverError<'_, PE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssumeIdResolverError::ProviderError(err) => {
                write!(f, "Error from assume provider: {err}")
            }
            AssumeIdResolverError::AliasNotFoundError(alias) => {
                write!(f, "Alias {alias} not found")
            }
        }
    }
}

impl<PE: std::error::Error> std::error::Error for AssumeIdResolverError<'_, PE> {}

pub fn resolve_assume_identifier<'c, 'p: 'c, A: ProvideAliases>(
    provider: &'p mut A,
    common: &'c CommonArgs,
) -> Result<AssumeIdentifier<'c>, AssumeIdResolverError<'c, A::Error>> {
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
            provider
                .load_aliases()
                .map_err(AssumeIdResolverError::ProviderError)?;
            provider
                .get_alias(l)
                .map_err(AssumeIdResolverError::ProviderError)?
                .ok_or(AssumeIdResolverError::AliasNotFoundError(l))
        }
        _ => unreachable!("Clap should prevent code from reaching this branch"),
    }
}
