use super::ProvideCredentials;
use crate::types::K8sExecCredentials;
use std::collections::HashMap;
use std::env;
use std::process::Command;

pub struct AwsCmdCredentialProvider {}

#[derive(Debug)]
pub enum Error {
    CommandFailed(std::io::Error),
    CommandExecFailed(String),
    InvalidCommandOutput(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::CommandFailed(err) => writeln!(f, "Command failed: {}", err),
            Error::CommandExecFailed(stderr) => {
                writeln!(f, "Command execution failed due to {}", stderr)
            }
            Error::InvalidCommandOutput(err) => {
                writeln!(f, "Serde failed to parse stdout to credentials: {}", err)
            }
        }
    }
}

impl std::error::Error for Error {}

impl ProvideCredentials for AwsCmdCredentialProvider {
    type Error = Error;
    async fn provide_credentials(
        &self,
        input: &super::ProvideCredentialsInput,
    ) -> Result<crate::types::K8sExecCredentials, Self::Error> {
        let filtered_envs: HashMap<String, String> =
            env::vars().filter(|(k, _)| !k.starts_with("AWS")).collect();
        let aws_sso_cmd = Command::new("aws-sso")
            .env_clear()
            .envs(&filtered_envs)
            .arg("exec")
            .arg("--account")
            .arg(&input.account_id)
            .arg("--role")
            .arg(&input.role)
            .arg("--")
            .arg("aws")
            .arg("--region")
            .arg(&input.region.to_string())
            .arg("eks")
            .arg("get-token")
            .arg("--cluster-name")
            .arg(&input.cluster)
            .arg("--output")
            .arg("json")
            .output()
            .map_err(Error::CommandFailed)?;

        let stderr = String::from_utf8_lossy(&aws_sso_cmd.stderr);
        let stdout = String::from_utf8_lossy(&aws_sso_cmd.stdout);

        if !aws_sso_cmd.status.success() {
            return Err(Error::CommandExecFailed(stderr.to_string()));
        }

        serde_json::from_str::<K8sExecCredentials>(&stdout).map_err(Error::InvalidCommandOutput)
    }
}
