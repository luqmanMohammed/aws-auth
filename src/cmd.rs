use std::collections::HashMap;
use std::env;
use std::fmt;

pub struct Arguments {
    pub account: String,
    pub role: String,
    pub region: String,
    pub cluster_name: String,
    pub cache_dir: String,
}

#[derive(Debug)]
pub enum Error {
    MissingArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MissingArgument(argument) => {
                writeln!(f, "Missing required argument: --{}", argument)
            }
        }
    }
}
impl std::error::Error for Error {}

impl TryFrom<Vec<String>> for Arguments {
    type Error = Error;
    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
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
        ) -> Result<&'a String, Error> {
            arg_map
                .get(arg)
                .ok_or_else(|| Error::MissingArgument(arg.to_string()))
        }

        Ok(Arguments {
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

impl Arguments {
    pub fn from_env_args() -> Result<Arguments, Error> {
        Arguments::try_from(env::args().collect::<Vec<_>>()[1..].to_vec())
    }
}
