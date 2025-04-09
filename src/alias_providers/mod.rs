use std::io;

use crate::common::AssumeIdentifier;
use json_alias_provider::JsonAliasProvider;
use std::path::Path;

pub type AliasProvider = JsonAliasProvider;
pub type AliasProviderError = io::Error;

pub trait ProvideAliases {
    type Error: std::error::Error;
    fn get_alias(&self, alias: &str) -> Result<Option<AssumeIdentifier>, Self::Error>;
    fn list_aliases(&self) -> Result<Vec<[&str; 3]>, Self::Error>;
    fn load_aliases(&mut self) -> Result<(), Self::Error>;
    fn set_alias(&mut self, alias: &str, account: &str, role: &str) -> Result<(), Self::Error>;
    fn unset_alias(&mut self, alias: &str) -> Result<(), Self::Error>;
}

pub fn build_alias_provider(config_dir: &Path) -> AliasProvider {
    JsonAliasProvider::new(config_dir.join("aliases.json"))
}

pub fn build_alias_provider_and_load(
    config_dir: &Path,
) -> Result<AliasProvider, AliasProviderError> {
    let mut provider = JsonAliasProvider::new(config_dir.join("aliases.json"));
    provider.load_aliases()?;
    Ok(provider)
}

pub mod json_alias_provider {

    use serde::{Deserialize, Serialize};

    use super::ProvideAliases;
    use crate::common::AssumeIdentifier;
    use std::collections::HashMap;
    use std::fs::File;
    use std::io;
    use std::path::PathBuf;

    #[derive(Serialize, Deserialize, Debug)]
    struct AccountRole {
        #[serde(rename = "accountId")]
        account: String,
        role: String,
    }

    #[derive(Debug)]
    pub struct JsonAliasProvider {
        file_path: PathBuf,
        aliases: HashMap<String, AccountRole>,
    }

    impl JsonAliasProvider {
        pub fn new(file_path: PathBuf) -> Self {
            JsonAliasProvider {
                file_path,
                aliases: HashMap::new(),
            }
        }
        fn save_aliases(&self) -> io::Result<()> {
            let file = File::create(&self.file_path)?;
            serde_json::to_writer(file, &self.aliases)?;
            Ok(())
        }
    }

    impl ProvideAliases for JsonAliasProvider {
        type Error = io::Error;

        fn load_aliases(&mut self) -> io::Result<()> {
            if self.file_path.exists() {
                let file = File::open(&self.file_path)?;
                let reader = io::BufReader::new(file);
                self.aliases = serde_json::from_reader::<
                    io::BufReader<File>,
                    HashMap<String, AccountRole>,
                >(reader)?;
            }
            Ok(())
        }

        fn set_alias(&mut self, alias: &str, account: &str, role: &str) -> Result<(), Self::Error> {
            let ai = AccountRole {
                account: account.to_string(),
                role: role.to_string(),
            };
            self.aliases.insert(alias.to_string(), ai);
            self.save_aliases()
        }

        fn unset_alias(&mut self, alias: &str) -> Result<(), Self::Error> {
            self.aliases.remove(alias);
            self.save_aliases()
        }

        fn list_aliases(&self) -> Result<Vec<[&str; 3]>, Self::Error> {
            Ok(self
                .aliases
                .iter()
                .map(|(alias, account_role)| {
                    [
                        alias.as_str(),
                        account_role.account.as_str(),
                        account_role.role.as_str(),
                    ]
                })
                .collect())
        }

        fn get_alias(&self, alias: &str) -> Result<Option<AssumeIdentifier>, Self::Error> {
            Ok(self.aliases.get(alias).map(|a| AssumeIdentifier {
                account: &a.account,
                role: &a.role,
            }))
        }
    }
}
