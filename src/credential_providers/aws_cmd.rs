use super::ProvideCredentials;
use crate::types::K8sExecCredentials;
use std::collections::HashMap;
use std::env;
use std::process::Command;

pub struct AwsCmdCredentialProvider {}

#[derive(Debug)]
pub enum ProviderAwsCmdError {
    CommandFailed(std::io::Error),
    CommandExecFailed(String),
    InvalidCommandOutput(serde_json::Error),
}

impl std::fmt::Display for ProviderAwsCmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderAwsCmdError::CommandFailed(err) => writeln!(f, "Command failed: {}", err),
            ProviderAwsCmdError::CommandExecFailed(stderr) => {
                writeln!(f, "Command execution failed due to {}", stderr)
            }
            ProviderAwsCmdError::InvalidCommandOutput(err) => {
                writeln!(f, "Serde failed to parse stdout to credentials: {}", err)
            }
        }
    }
}

impl std::error::Error for ProviderAwsCmdError {}

impl ProvideCredentials for AwsCmdCredentialProvider {
    type Error = ProviderAwsCmdError;
    async fn provide_credentials(
        self,
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
            .map_err(ProviderAwsCmdError::CommandFailed)?;

        let stderr = String::from_utf8_lossy(&aws_sso_cmd.stderr);
        let stdout = String::from_utf8_lossy(&aws_sso_cmd.stdout);

        if !aws_sso_cmd.status.success() {
            return Err(ProviderAwsCmdError::CommandExecFailed(stderr.to_string()));
        }

        serde_json::from_str::<K8sExecCredentials>(&stdout)
            .map_err(ProviderAwsCmdError::InvalidCommandOutput)
    }
}
