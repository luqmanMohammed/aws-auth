use crate::cmd::Args;
use std::collections::HashMap;
use std::env;
use std::process::Command;

pub struct CredsResolverError {
    message: String,
}

impl std::fmt::Display for CredsResolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to resolve credentials: {}", self.message)
    }
}

pub trait ResolveCreds {
    fn resolve_creds(&self, args: &Args) -> Result<String, CredsResolverError>;
}

pub fn resolve_exec_credentials<T: ResolveCreds>(
    resolver: T,
    args: &Args,
) -> Result<String, CredsResolverError> {
    Ok(resolver.resolve_creds(args)?.trim().to_string())
}

pub struct OidcCmdResolver {}

impl ResolveCreds for OidcCmdResolver {
    fn resolve_creds(&self, args: &Args) -> Result<String, CredsResolverError> {
        let filtered_envs: HashMap<String, String> =
            env::vars().filter(|(k, _)| !k.starts_with("AWS")).collect();
        let aws_sso_cmd = Command::new("aws-sso")
            .env_clear()
            .envs(&filtered_envs)
            .arg("exec")
            .arg("--account")
            .arg(&args.account)
            .arg("--role")
            .arg(&args.role)
            .arg("--")
            .arg("aws")
            .arg("--region")
            .arg(&args.region)
            .arg("eks")
            .arg("get-token")
            .arg("--cluster-name")
            .arg(&args.cluster_name)
            .arg("--output")
            .arg("json")
            .output()
            .map_err(|err| CredsResolverError {
                message: err.to_string(),
            })?;

        let stderr = String::from_utf8_lossy(&aws_sso_cmd.stderr);
        let stdout = String::from_utf8_lossy(&aws_sso_cmd.stdout);

        if !aws_sso_cmd.status.success() {
            return Err(CredsResolverError {
                message: format!("Command failed: {}", stderr),
            });
        }

        Ok(stdout.to_string())
    }
}
