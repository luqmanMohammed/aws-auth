use aws_config::imds::client::error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type RepositoryBackend = mono_json_backend::MonoJsonBackend;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct SsmHost {
    pub instance_id: Option<String>,
    pub account: Option<String>,
    pub role: Option<String>,
    pub region: Option<String>,
    pub alias: Option<String>,
    pub default_remote_port: Option<u16>,
    pub default_local_port: Option<u16>,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SsmHostId(String);

impl From<String> for SsmHostId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SsmHostId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl std::fmt::Display for SsmHostId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SsmHostError {
    #[error(
        "alias and account/role pair is mutualy exclusive, account and role require each other"
    )]
    InvalidInput,
}

impl SsmHost {
    pub fn new(
        instance_id: Option<String>,
        account: Option<String>,
        role: Option<String>,
        region: Option<String>,
        alias: Option<String>,
        default_remote_port: Option<u16>,
        default_local_port: Option<u16>,
    ) -> Result<Self, SsmHostError> {
        let has_account = account.is_some();
        let has_role = role.is_some();
        let has_alias = alias.is_some();

        if (has_account != has_role) || (has_alias && (has_account || has_role)) {
            return Err(SsmHostError::InvalidInput);
        }
        Ok(Self {
            instance_id,
            account,
            role,
            region,
            alias,
            default_remote_port,
            default_local_port,
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct SsmHosts(HashMap<SsmHostId, SsmHost>);

pub trait SsmHostRepositoryBackend {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_hosts(&self) -> Result<SsmHosts, Self::Error>;
    fn commit_hosts(&self, hosts: &SsmHosts) -> Result<(), Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError<B: SsmHostRepositoryBackend> {
    #[error("Failed to perform repository operation: {0}")]
    BackendError(B::Error),
    #[error("Host '{0}' already exists")]
    HostAlreadyExists(SsmHostId),
    #[error("Host '{0}' not found")]
    HostNotFound(SsmHostId),
}

pub struct SsmHostRepository<B: SsmHostRepositoryBackend> {
    backend: B,
    hosts: SsmHosts,
}

impl<B: SsmHostRepositoryBackend> SsmHostRepository<B> {
    pub fn new(backend: B) -> Result<Self, RepositoryError<B>> {
        let hosts = backend
            .load_hosts()
            .map_err(RepositoryError::BackendError)?;
        Ok(Self { backend, hosts })
    }
}

impl<B: SsmHostRepositoryBackend> SsmHostRepository<B> {
    pub fn add_host(
        &mut self,
        id: SsmHostId,
        host: SsmHost,
        overwrite: bool,
    ) -> Result<(), RepositoryError<B>> {
        if !overwrite && self.hosts.0.contains_key(&id) {
            return Err(RepositoryError::HostAlreadyExists(id));
        }
        self.hosts.0.insert(id, host);
        self.backend
            .commit_hosts(&self.hosts)
            .map_err(RepositoryError::BackendError)
    }

    pub fn remove_host(&mut self, id: &SsmHostId) -> Result<(), RepositoryError<B>> {
        if self.hosts.0.remove(id).is_none() {
            return Err(RepositoryError::HostNotFound(id.clone()));
        }
        self.backend
            .commit_hosts(&self.hosts)
            .map_err(RepositoryError::BackendError)
    }

    pub fn get_host(&self, id: &SsmHostId) -> Option<&SsmHost> {
        self.hosts.0.get(id)
    }

    pub fn list_hosts(&self) -> &HashMap<SsmHostId, SsmHost> {
        &self.hosts.0
    }
}

pub mod mono_json_backend {
    use super::*;
    use serde_json;
    use std::fs::File;
    use std::path::PathBuf;

    #[derive(Debug)]
    pub struct MonoJsonBackend {
        file_path: PathBuf,
    }

    impl MonoJsonBackend {
        pub fn new(file_path: PathBuf) -> Self {
            Self { file_path }
        }

        pub fn new_from_config_dir(config_dir: &std::path::Path) -> Self {
            Self::new(config_dir.join("ssm_hosts.json"))
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum MonoJsonBackendError {
        #[error("ssm hosts file error: {0}")]
        Io(#[from] std::io::Error),
        #[error("error parsing ssm hosts: {0}")]
        Json(#[from] serde_json::Error),
    }

    impl SsmHostRepositoryBackend for MonoJsonBackend {
        type Error = MonoJsonBackendError;

        fn load_hosts(&self) -> Result<SsmHosts, Self::Error> {
            if !self.file_path.exists() {
                return Ok(SsmHosts(HashMap::new()));
            }
            let file = File::open(&self.file_path)?;
            let hosts = serde_json::from_reader(file)?;
            Ok(hosts)
        }

        fn commit_hosts(&self, hosts: &SsmHosts) -> Result<(), Self::Error> {
            let tmp = self.file_path.with_extension("json.tmp");
            let file = File::create(&tmp)?;
            serde_json::to_writer_pretty(file, hosts)?;
            std::fs::rename(&tmp, &self.file_path)?;
            Ok(())
        }
    }
}
