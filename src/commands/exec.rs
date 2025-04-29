use crate::credential_providers::{ProvideCredentials, ProvideCredentialsInput};
use aws_config::Region;
use std::collections::HashMap;
use std::io;
use std::process::{Command, Stdio};

pub struct ExecExecInputs {
    pub region: Region,
    pub arguments: Vec<String>,
}

#[derive(Debug)]
pub enum Error<PE>
where
    PE: std::fmt::Debug + std::error::Error,
{
    Provider(PE),
    InvalidCommand(String),
    ProgramSpawnFailed(io::Error),
    ProgramExecFailed(io::Error),
}

impl<PE: std::error::Error> std::error::Error for Error<PE> {}
impl<PE: std::error::Error> std::fmt::Display for Error<PE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Provider(err) => writeln!(f, "Error generating AWS Auth Credentials: {}", err),
            Error::InvalidCommand(err) => writeln!(f, "Invalid command: {}", err),
            Error::ProgramSpawnFailed(err) => writeln!(f, "Failed to start program: {}", err),
            Error::ProgramExecFailed(err) => {
                writeln!(f, "Program failed during execution: {}", err)
            }
        }
    }
}

pub type Result<PE> = std::result::Result<(), Error<PE>>;

pub async fn exec_exec<P: ProvideCredentials>(
    credentials_provider: P,
    provider_input: &ProvideCredentialsInput,
    exec_inputs: ExecExecInputs,
) -> Result<P::Error> {
    let credentials = credentials_provider
        .provide_credentials(provider_input)
        .await
        .map_err(Error::Provider)?;

    let program = exec_inputs
        .arguments
        .first()
        .ok_or(Error::InvalidCommand("Missing Program".to_string()))?;
    let args = &(exec_inputs.arguments)[1..];

    let mut envs = HashMap::new();

    envs.insert("AWS_REGION", exec_inputs.region.as_ref());
    envs.insert("AWS_DEFAULT_REGION", exec_inputs.region.as_ref());
    envs.insert("AWS_ACCESS_KEY_ID", credentials.access_key_id());
    envs.insert("AWS_SECRET_ACCESS_KEY", credentials.secret_access_key());
    envs.insert(
        "AWS_SESSION_TOKEN",
        credentials.session_token().unwrap_or(""),
    );

    let mut child = Command::new(program)
        .args(args)
        .envs(envs)
        .stdin(Stdio::inherit())
        .stderr(io::stderr())
        .stdout(io::stdout())
        .spawn()
        .map_err(Error::ProgramSpawnFailed)?;

    child.wait().map_err(Error::ProgramExecFailed)?;

    Ok(())
}
