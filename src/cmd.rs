use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// CLI tool for AWS authentication and credential management
#[derive(Parser)]
#[command(about, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

// Common
const ARG_SHORT_ACCOUNT: char = 'a';
const ARG_SHORT_ROLE: char = 'r';
const ARG_SHORT_ALIAS: char = 'A';
const ARG_SHORT_CONFIG_DIR: char = 'C';
const ARG_SHORT_IGNORE_CACHE: char = 'i';
const ARG_SHORT_REFRESH_STS_TOKEN: char = 't';
const ARG_SHORT_REGION: char = 'R';
// Eks
const ARG_SHORT_CLUSTER: char = 'c';

/// Output format for command results
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum OutputFormat {
    /// JSON formatted output
    Json,
    /// Plain text formatted output
    Text,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Text => write!(f, "text"),
        }
    }
}

fn validate_account_id(s: &str) -> Result<String, String> {
    if s.len() != 12 {
        return Err(format!(
            "AWS Account ID must be exactly 12 digits, got {}",
            s.len()
        ));
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return Err("AWS Account ID must contain only digits".to_string());
    }
    Ok(s.to_string())
}

#[derive(Args, Clone)]
#[group(required = true, multiple = true)]
pub struct AssumeInput {
    /// AWS Account ID to authenticate against.
    #[arg(short = ARG_SHORT_ACCOUNT, long, requires="role", conflicts_with="alias", value_parser=validate_account_id)]
    pub account: Option<String>,

    /// AWS IAM Role to assume during authentication.
    #[arg(short = ARG_SHORT_ROLE, long, requires="account", conflicts_with="alias")]
    pub role: Option<String>,

    /// Predefined alias for an account and role combination
    #[arg(short = ARG_SHORT_ALIAS, long, conflicts_with="account", conflicts_with="role")]
    pub alias: Option<String>,
}

/// Common arguments used across multiple commands
#[derive(Args)]
pub struct CommonArgs {
    /// Input parameters for assuming an AWS role
    #[command(flatten)]
    pub assume_input: AssumeInput,

    /// Optional cache directory for storing authentication tokens.
    /// If not provided, the default cache location will be used.
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,

    /// Optional config path to retrieve AWS Auth Config.
    /// If not provided, the default config path will be used
    #[arg(short = ARG_SHORT_CONFIG_DIR, long, env = "AWS_AUTH_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// Flag to ignore the cache and request new credentials even if cached ones are available.
    /// Defaults to `false`.
    #[arg(short = ARG_SHORT_IGNORE_CACHE, long, default_value_t = false)]
    pub ignore_cache: bool,

    /// Flag to refresh the session token even if it is still valid.
    /// Defaults to `false`.
    #[arg(short = ARG_SHORT_REFRESH_STS_TOKEN, long, default_value_t = false)]
    pub refresh_sts_token: bool,

    /// The AWS region to export as default and selected region.
    /// If not provided, it defaults to `eu-west-2`.
    #[arg(short = ARG_SHORT_REGION, long, default_value_t=String::from("eu-west-2"))]
    pub region: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// The `Init` subcommand is used to initialize the AWS SSO configuration.
    /// The configuration will be saved to the default configuration file location, or the location specified by the user.
    /// If the configuration directory already exists, the user can choose to recreate it.
    /// Default configuration directory: `$HOME/.aws-auth`
    Init {
        /// The SSO start URL for the AWS account.
        #[arg(short, long)]
        sso_start_url: String,
        /// The AWS region where the SSO service is hosted.
        #[arg(short = 'r', long)]
        sso_region: String,
        /// The maximum number of attempts to authenticate with AWS SSO.
        #[arg(short, long)]
        max_attempts: Option<usize>,
        /// The initial delay in secounds before retrying the authentication process.
        #[arg(short, long)]
        initial_delay_secounds: Option<u64>,
        /// The retry interval in secounds between each authentication attempt.
        #[arg(short = 't', long)]
        retry_interval_secounds: Option<u64>,
        /// Optional directory to store the AWS SSO configuration. If not provided, the default directory will be used.
        #[arg(short, long)]
        config_dir: Option<PathBuf>,
        /// Flag to recreate the configuration directory if it already exists.
        #[arg(short = 'e', long, default_value_t = false)]
        recreate: bool,
    },
    /// The `Eks` subcommand is used to print a valid Kubernetes authentication object
    /// to be used with the Kubernetes external authentication process.
    /// This is particularly useful when authenticating with an AWS EKS cluster.
    Eks {
        #[clap(flatten)]
        common: CommonArgs,

        /// The name of the EKS cluster for which to generate the authentication object.
        #[arg(short = ARG_SHORT_CLUSTER, long)]
        cluster: String,

        /// Optional cache directory for storing EKS authentication tokens.
        /// If not specified, a default cache location is used.
        #[arg(long)]
        eks_cache_dir: Option<PathBuf>,

        /// Optional EKS auth token TTL in secounds.
        /// If not specified, default value of `900` secounds (15m) will be used.
        #[arg(long)]
        eks_expiry_seconds: Option<usize>,
    },

    /// The `Eval` subcommand is used to print AWS environment variables.
    /// These variables can be used in shell `eval` commands to set up
    /// the AWS environment for subsequent commands or scripts.
    Eval {
        #[clap(flatten)]
        common: CommonArgs,
    },

    /// The `Exec` subcommand is used to execute the provided command
    /// with AWS credentials.
    /// This allows you to execute external commands such as AWS CLI commands
    /// with the appropriate AWS credentials.
    Exec {
        #[clap(flatten)]
        common: CommonArgs,

        /// The command and its arguments to be executed with the AWS credentials.
        /// You must provide the command after `--`.
        #[arg(trailing_var_arg = true)]
        arguments: Vec<String>,
    },

    /// The `Alias` subcommand is used to manage AWS account aliases.
    /// You can set, unset, or list account aliases.
    Alias {
        #[clap(subcommand)]
        subcommand: Alias,
    },
}

#[derive(Args)]
pub struct AliasCommonArgs {
    /// Optional config path to retrieve AWS Auth Config.
    /// If not provided, the default config path will be used
    #[arg(short = ARG_SHORT_CONFIG_DIR, long, env = "AWS_AUTH_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,
}

/// Subcommands for alias management
#[derive(Subcommand)]
pub enum Alias {
    /// The `Set` subcommand is used to set an alias for a specific AWS account and role.
    /// This allows you to easily reference AWS accounts and roles using a friendly name
    /// instead of the full account ID and role name.
    Set {
        /// Common alias management arguments
        #[clap(flatten)]
        common: AliasCommonArgs,
        /// The alias name to set
        alias: String,
        /// AWS Account ID to map to the alias
        #[arg(short = ARG_SHORT_ACCOUNT, long, value_parser=validate_account_id)]
        account: String,
        /// AWS IAM Role name to map to the alias
        #[arg(short = ARG_SHORT_ROLE, long)]
        role: String,
        /// Overwrite an alias if it already exits
        #[arg(short = 'w', long, default_value_t = false)]
        overwrite: bool,
    },
    /// The `Unset` subcommand is used to remove an alias for a specific AWS account and role.
    Unset {
        /// Common alias management arguments
        #[clap(flatten)]
        common: AliasCommonArgs,
        /// The alias name to remove
        alias: String,
    },
    /// The `List` subcommand is used to list all aliases for AWS accounts and roles.
    List {
        /// Common alias management arguments
        #[clap(flatten)]
        common: AliasCommonArgs,
        /// Format for the output list
        #[arg(short = 'O', long, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
        /// Flag to omit headers in the output
        #[arg(short = 'n', long, default_value_t = false)]
        no_headers: bool,
        /// Fields to omit from the output
        #[arg(short = 'i', long, value_delimiter = ',')]
        omit_fields: Vec<String>,
    },
}
