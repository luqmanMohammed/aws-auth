use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::env;

pub struct Args {
    pub account: String,
    pub role: String,
    pub region: String,
    pub cluster_name: String,
    pub cache_dir: String,
}

#[derive(Debug)]
pub struct ArgParseError {
    message: String,
}
impl fmt::Display for ArgParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Failed to convert args into EKS Auth Args: {}",
            self.message
        )
    }
}
impl Error for ArgParseError {}

impl TryFrom<Vec<String>> for Args {
    type Error = ArgParseError;
    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        if value.len() % 2 != 0 {
            return Err(ArgParseError {
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
        ) -> Result<&'a String, ArgParseError> {
            arg_map.get(arg).ok_or_else(|| ArgParseError {
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

impl Args {
    pub fn from_env_args() -> Result<Args, ArgParseError> {
        Args::try_from(env::args().collect::<Vec<_>>()[1..].to_vec())
    }
}