use crate::cmd::Ssm;
use crate::ssm::host::{
    RepositoryBackend, RepositoryError, SsmHost, SsmHostError, SsmHostRepository,
};
use crate::utils::formatters::{json::JsonFormatter, text::TextFormatter, TabularFormatter};
use crate::utils::resolve_config_dir;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error with SSM Repository backend: {0}")]
    Respository(#[from] RepositoryError<RepositoryBackend>),
    #[error("Error formating aliases list using json output: {0}")]
    JsonFormatter(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    SsmHostError(#[from] SsmHostError),
}

pub async fn exec_ssm(subcommand: Ssm) -> Result<(), Error> {
    let config_dir = resolve_config_dir(subcommand.config_dir());
    let mut host_provider =
        SsmHostRepository::new(RepositoryBackend::new_from_config_dir(&config_dir))?;

    match subcommand {
        Ssm::Connect { common } | Ssm::PortForward { common, .. } => Ok(()),
        Ssm::SaveHost {
            name,
            instance_id,
            account,
            role,
            alias,
            region,
            default_remote_port,
            default_local_port,
            overwrite,
            ..
        } => {
            let host = SsmHost::new(
                instance_id,
                account,
                role,
                region,
                alias,
                default_remote_port,
                default_local_port,
            )?;
            host_provider.add_host(name.into(), host, overwrite)?;
            Ok(())
        }
        Ssm::RemoveHost { name, .. } => {
            host_provider.remove_host(&name.into())?;
            Ok(())
        }
        Ssm::ListHosts { formatting, .. } => {
            let host_map = host_provider.list_hosts();
            let hosts: Vec<Vec<String>> = host_map
                .iter()
                .map(|(id, h)| {
                    vec![
                        id.to_string(),
                        h.instance_id.clone().unwrap_or("Not Set".into()),
                        h.account.clone().unwrap_or("Not Set".into()),
                        h.role.clone().unwrap_or("Not Set".into()),
                        h.region.clone().unwrap_or("Not Set".into()),
                        h.alias.clone().unwrap_or("Not Set".into()),
                        h.default_remote_port.unwrap_or(0).to_string(),
                        h.default_local_port.unwrap_or(0).to_string(),
                    ]
                })
                .collect();

            let omit_fields = formatting.omit_fields.iter().map(|v| v.as_str()).collect();

            match formatting.output {
                crate::cmd::OutputFormat::Json => {
                    let formatter = JsonFormatter::new(omit_fields, formatting.no_headers);
                    let output = formatter.format(
                        &[
                            "host",
                            "instanceID",
                            "account",
                            "role",
                            "region",
                            "alias",
                            "defaultRemotePort",
                            "defaultLocalPort",
                        ],
                        hosts,
                    )?;
                    println!("{}", output)
                }
                crate::cmd::OutputFormat::Text => {
                    let formatter = TextFormatter::new(omit_fields, formatting.no_headers, " | ");
                    let output = formatter
                        .format(
                            &[
                                "Host",
                                "Instance Id",
                                "Account Id",
                                "Role",
                                "Region",
                                "Alias",
                                "Default Remote Port",
                                "Default Local Port",
                            ],
                            hosts,
                        )
                        .expect("TextFormatter doesnt error. Returns result to satisfy trait");
                    println!("{}", output)
                }
            }
            Ok(())
        }
    }
}
