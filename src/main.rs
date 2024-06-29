use chrono::{DateTime, Duration, Utc};
use serde::{self, Deserialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

#[derive(Debug)]
struct Args {
    account: String,
    role: String,
    region: String,
    cluster_name: String,
    cache_dir: String,
}

#[derive(Debug, Deserialize)]
struct K8sExecCredential {
    status: K8sExecCredentialStatus,
}

#[derive(Debug, Deserialize)]
struct K8sExecCredentialStatus {
    #[serde(alias = "expirationTimestamp")]
    expiration_timestamp: DateTime<Utc>,
}

#[derive(Debug)]
struct CmdConversionError {
    message: String,
}
impl fmt::Display for CmdConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Failed to convert args into EKS Auth Args: {}",
            self.message
        )
    }
}
impl Error for CmdConversionError {}

impl TryFrom<Vec<String>> for Args {
    type Error = CmdConversionError;
    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        if value.len() % 2 != 0 {
            return Err(CmdConversionError {
                message: String::from("Invalid number of arguments"),
            });
        }

        let mut arg_map: HashMap<String, String> = HashMap::new();
        let mut key: Option<String> = None;
        for (i, arg) in value.into_iter().enumerate() {
            if i % 2 == 0 && arg.starts_with("--") {
                key = Some(arg[2..].to_string());
            } else if let Some(k) = key.take() {
                arg_map.insert(k, arg);
            }
        }

        fn safe_get_arg<'a>(
            arg_map: &'a HashMap<String, String>,
            arg: &str,
        ) -> Result<&'a String, CmdConversionError> {
            arg_map.get(arg).ok_or_else(|| CmdConversionError {
                message: format!("Required argument '--{}' missing", arg),
            })
        }

        Ok(Args {
            account: safe_get_arg(&arg_map, "account")?.to_string(),
            role: safe_get_arg(&arg_map, "role")?.to_string(),
            region: safe_get_arg(&arg_map, "region")?.to_string(),
            cluster_name: safe_get_arg(&arg_map, "cluster-name")?.to_string(),
            cache_dir: match arg_map.get("cache-dir") {
                None => "/tmp",
                Some(v) => v,
            }
            .to_string(),
        })
    }
}

fn get_exec_credentials(args: &Args) -> Result<String, ()> {
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
        .map_err(|err| {
            eprintln!("ERROR: Command execution failed: {}", err);
        })?;

    let stderr = String::from_utf8_lossy(&aws_sso_cmd.stderr);
    let stdout = String::from_utf8_lossy(&aws_sso_cmd.stdout);

    if !aws_sso_cmd.status.success() {
        eprintln!("ERROR: Command failed: {}", stderr);
        return Err(());
    }

    Ok(stdout.to_string())
}

fn resolve_cache_hit(cache_path: &Path) -> Option<String> {
    fs::read_to_string(cache_path).ok().and_then(|content| {
        serde_json::from_str::<K8sExecCredential>(&content)
            .ok()
            .and_then(|k8s_exec_creds| {
                if Utc::now() + Duration::seconds(30) < k8s_exec_creds.status.expiration_timestamp {
                    Some(content)
                } else {
                    None
                }
            })
    })
}

fn main() -> Result<(), ()> {
    let cmd_args = env::args().collect::<Vec<_>>()[1..].to_vec();
    let args = Args::try_from(cmd_args).map_err(|err| eprintln!("ERROR: {}", err))?;

    let cache_file_name = format!(
        "eks-{account}-{role}-{region}-{cluster}",
        account = &args.account,
        role = &args.role,
        region = &args.region,
        cluster = &args.cluster_name
    );

    let mut cache_path = PathBuf::new();
    cache_path.push(&args.cache_dir);
    cache_path.push(cache_file_name);

    let exec_creds = match resolve_cache_hit(&cache_path) {
        Some(hit) => hit,
        None => {
            let creds = get_exec_credentials(&args)?;
            fs::write(cache_path, &creds)
                .map_err(|err| eprintln!("ERROR: Unable to create cache file {}", err))?;
            creds
        }
    };

    println!("{}", exec_creds.trim());

    Ok(())
}
