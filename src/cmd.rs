use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// AWS-Auth: A CLI tool for AWS authentication and credential management
///
/// Manages AWS credentials, SSO sessions, role assumptions, and provides integrations
/// with services like EKS. Simplifies credential workflows across multiple accounts.
#[derive(Parser)]
#[command(about, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

// Common argument short flags
const ARG_SHORT_ACCOUNT: char = 'a';
const ARG_SHORT_ROLE: char = 'r';
const ARG_SHORT_ALIAS: char = 'A';
const ARG_SHORT_CONFIG_DIR: char = 'C';
const ARG_SHORT_IGNORE_CACHE: char = 'i';
const ARG_SHORT_REFRESH_STS_TOKEN: char = 't';
const ARG_SHORT_REGION: char = 'R';
// EKS-specific argument short flag
const ARG_SHORT_CLUSTER: char = 'c';

/// Defines output format options for command results
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum OutputFormat {
    /// JSON formatted output for programmatic consumption
    Json,
    /// Plain text formatted output for human readability
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
    /// AWS Account ID to authenticate against (must be 12 digits)
    #[arg(short = ARG_SHORT_ACCOUNT, long, requires="role", conflicts_with="alias", value_parser=validate_account_id)]
    pub account: Option<String>,

    /// AWS IAM Role to assume during authentication
    #[arg(short = ARG_SHORT_ROLE, long, requires="account", conflicts_with="alias")]
    pub role: Option<String>,

    /// Predefined alias for an account and role combination
    /// Use instead of specifying account and role separately
    #[arg(short = ARG_SHORT_ALIAS, long, conflicts_with="account", conflicts_with="role")]
    pub alias: Option<String>,
}

/// Common arguments shared across multiple commands
#[derive(Args)]
pub struct CommonArgs {
    /// Input parameters for assuming an AWS role
    #[command(flatten)]
    pub assume_input: AssumeInput,

    /// Custom directory for storing SSO authentication tokens
    /// Defaults to standard AWS SSO cache location if not specified
    /// Default: Value specified for config-dir
    #[arg(long)]
    pub sso_cache_dir: Option<PathBuf>,

    /// Custom directory for AWS Auth configuration
    /// Can be set via AWS_AUTH_CONFIG_DIR environment variable
    /// Default: ~/.aws-auth
    #[arg(short = ARG_SHORT_CONFIG_DIR, long, env = "AWS_AUTH_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// Force new credential retrieval instead of using cached credentials
    /// Default: false (use cached credentials when available)
    #[arg(short = ARG_SHORT_IGNORE_CACHE, long, default_value_t = false)]
    pub ignore_cache: bool,

    /// Force refresh of the STS token even if current token is valid
    /// Default: false (use existing valid token)
    #[arg(short = ARG_SHORT_REFRESH_STS_TOKEN, long, default_value_t = false)]
    pub refresh_sts_token: bool,

    /// AWS region to use for operations
    /// Default: eu-west-2
    #[arg(short = ARG_SHORT_REGION, long, default_value_t=String::from("eu-west-2"))]
    pub region: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize AWS SSO configuration
    ///
    /// Creates or updates the AWS SSO configuration used by aws-auth.
    /// Stores settings in the specified (or default) configuration directory.
    /// Default location: $HOME/.aws-auth
    Init {
        /// SSO start URL for AWS Identity Center (e.g., https://my-company.awsapps.com/start)
        #[arg(short, long)]
        sso_start_url: String,

        /// AWS region where the SSO service is hosted (e.g., us-east-1)
        #[arg(short = 'r', long)]
        sso_region: String,

        /// Maximum authentication retry attempts
        /// Default: 10
        #[arg(short, long)]
        max_attempts: Option<usize>,

        /// Initial delay in seconds before first retry attempt
        /// Default: 10s
        #[arg(short, long)]
        initial_delay_secounds: Option<u64>,

        /// Interval in seconds between retry attempts
        /// Default: 5s
        #[arg(short = 't', long)]
        retry_interval_secounds: Option<u64>,

        /// Custom directory to store the AWS SSO configuration
        /// Default: $HOME/.aws-auth
        #[arg(short, long)]
        config_dir: Option<PathBuf>,

        /// Recreate configuration directory if it already exists
        /// Default: false (preserve existing configuration)
        #[arg(short = 'e', long, default_value_t = false)]
        recreate: bool,
    },

    #[clap(flatten)]
    Core(CoreCommands),

    /// Manage AWS account aliases
    ///
    /// Create, update, remove, or list aliases that map to account ID and role combinations
    /// for simplified credential access.
    Alias {
        #[clap(subcommand)]
        subcommand: Alias,
    },

    /// Manage AWS SSO accounts and roles
    ///
    /// List available accounts and roles accessible through AWS SSO.
    /// Helps users discover available resources within their organization.
    Sso {
        #[clap(subcommand)]
        subcommand: Sso,
    },

    /// Execute operations across multiple AWS accounts
    ///
    /// Run commands in sequential or parallel mode across multiple accounts and roles.
    /// Useful for multi-account administrative tasks.
    Batch {
        #[clap(subcommand)]
        subcommand: Batch,
    },
}

#[derive(Subcommand)]
pub enum CoreCommands {
    /// Generate Kubernetes authentication configuration for AWS EKS
    ///
    /// Outputs kubectl authentication objects for use with EKS clusters.
    /// Enables kubectl to authenticate via AWS IAM credentials.
    Eks {
        #[clap(flatten)]
        common: CommonArgs,

        /// Name of the EKS cluster to generate authentication for
        #[arg(short = ARG_SHORT_CLUSTER, long)]
        cluster: String,

        /// Custom directory for storing EKS authentication tokens
        /// Default: <Value specified for config-dir>/eks
        #[arg(long)]
        eks_cache_dir: Option<PathBuf>,

        /// Token expiration time in seconds
        /// Default: 900 seconds (15 minutes)
        #[arg(long)]
        eks_expiry_seconds: Option<usize>,
    },

    /// Output AWS environment variables for credential access
    ///
    /// Prints environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, etc.)
    /// for use with shell evaluation (eval) to configure credentials in current shell.
    Eval {
        #[clap(flatten)]
        common: CommonArgs,
    },

    /// Execute a command with AWS credentials
    ///
    /// Runs the specified command with AWS credentials injected into its environment.
    /// Useful for running tools that require AWS authentication.
    Exec {
        #[clap(flatten)]
        common: CommonArgs,

        /// Command and arguments to execute with AWS credentials
        /// Must be provided after -- separator
        /// Example: aws-auth exec -a 123456789012 -r AdminRole -- aws s3 ls
        #[arg(trailing_var_arg = true)]
        arguments: Vec<String>,
    },
}

impl CoreCommands {
    pub fn get_common_args(&self) -> &CommonArgs {
        match self {
            CoreCommands::Eks { common, .. } => common,
            CoreCommands::Eval { common, .. } => common,
            CoreCommands::Exec { common, .. } => common,
        }
    }
}

#[derive(Args)]
pub struct FormatCommonArgs {
    /// Output format type
    /// Options: json, text (default: text)
    #[arg(short = 'F', long, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,

    /// Remove column headers from output
    /// Default: false (include headers)
    #[arg(short = 'H', long, default_value_t = false)]
    pub no_headers: bool,

    /// Fields to exclude from output
    /// Comma-separated list of field names
    #[arg(short = 'O', long, value_delimiter = ',')]
    pub omit_fields: Vec<String>,
}

#[derive(Args)]
pub struct AliasCommonArgs {
    /// Custom directory for AWS Auth configuration
    /// Can be set via AWS_AUTH_CONFIG_DIR environment variable
    #[arg(short = ARG_SHORT_CONFIG_DIR, long, env = "AWS_AUTH_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,
}

/// Subcommands for alias management
#[derive(Subcommand)]
pub enum Alias {
    /// Create or update an alias for AWS account and role
    ///
    /// Maps a user-friendly name to a specific AWS account ID and IAM role
    /// for easier reference in subsequent commands.
    Set {
        /// Common alias management arguments
        #[clap(flatten)]
        common: AliasCommonArgs,

        /// Name of the alias to create or update
        alias: String,

        /// AWS Account ID to associate with this alias (12 digits)
        #[arg(short = ARG_SHORT_ACCOUNT, long, value_parser=validate_account_id)]
        account: String,

        /// AWS IAM Role name to associate with this alias
        #[arg(short = ARG_SHORT_ROLE, long)]
        role: String,

        /// Replace existing alias if one exists with the same name
        /// Default: false (prevents accidental overwrites)
        #[arg(short = 'w', long, default_value_t = false)]
        overwrite: bool,
    },

    /// Delete an alias
    ///
    /// Removes a previously created alias from configuration.
    Unset {
        /// Common alias management arguments
        #[clap(flatten)]
        common: AliasCommonArgs,

        /// Name of the alias to remove
        alias: String,
    },

    /// Display configured aliases
    ///
    /// Shows all defined aliases with their associated account IDs and roles.
    List {
        /// Common alias management arguments
        #[clap(flatten)]
        common: AliasCommonArgs,

        /// Optional formatting arguments for the output
        #[clap(flatten)]
        formatting: FormatCommonArgs,
    },
}

#[derive(Args)]
pub struct SsoCommonArgs {
    /// Custom directory for storing SSO authentication tokens
    /// Default: Value specified for config-dir
    #[arg(long)]
    pub sso_cache_dir: Option<PathBuf>,

    /// Custom directory for AWS Auth configuration
    /// Can be set via AWS_AUTH_CONFIG_DIR environment variable
    /// Default: ~/.aws-auth
    #[arg(short = ARG_SHORT_CONFIG_DIR, long, env = "AWS_AUTH_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// Force new credential retrieval instead of using cached credentials
    /// Default: false (use cached credentials when available)
    #[arg(short = ARG_SHORT_IGNORE_CACHE, long, default_value_t = false)]
    pub ignore_cache: bool,
}

/// Subcommands for AWS SSO management
#[derive(Subcommand)]
pub enum Sso {
    /// Display available AWS accounts
    ///
    /// Lists all accounts accessible through AWS SSO with account IDs and names.
    ListAccounts {
        #[clap(flatten)]
        common: SsoCommonArgs,

        /// Optional formatting arguments for the output
        #[clap(flatten)]
        formatting: FormatCommonArgs,
    },

    /// Display available roles for a specific AWS account
    ///
    /// Lists all IAM roles available for the specified account through AWS SSO.
    ListAccountRoles {
        #[clap(flatten)]
        common: SsoCommonArgs,

        /// AWS Account ID to list roles for (12 digits)
        #[arg(short = ARG_SHORT_ACCOUNT, long, value_parser=validate_account_id)]
        account: String,

        /// Optional formatting arguments for the output
        #[clap(flatten)]
        formatting: FormatCommonArgs,
    },
}

#[derive(Args)]
pub struct BatchCommonArgs {
    /// AWS Account IDs to target (comma-separated list)
    /// Ignored when using aliases or regex filters
    #[arg(short = 'a', long, value_delimiter = ',')]
    pub account_ids: Option<Vec<String>>,

    /// IAM roles to attempt in priority order
    /// First successful role will be used for operations
    #[arg(short = 'R', long)]
    pub role_order: Option<Vec<String>>,

    /// Target accounts by configured aliases (comma-separated list)
    #[arg(short = 'A', long, value_delimiter = ',')]
    pub aliases: Option<Vec<String>>,

    /// Filter accounts by name using regular expression pattern
    #[arg(short = 'f', long)]
    pub account_filter_regex: Option<String>,

    /// AWS region for operations
    /// Default: eu-west-2
    #[arg(short = ARG_SHORT_REGION, long, default_value_t=String::from("eu-west-2"))]
    pub region: String,

    /// Number of concurrent operations to perform
    /// Default: 1 (sequential processing)
    #[arg(short = 'p', long, default_value_t = 1)]
    pub parallel: usize,

    /// Custom directory for storing SSO authentication tokens
    /// Default: Value specified for config-dir
    #[arg(long)]
    pub sso_cache_dir: Option<PathBuf>,

    /// Custom directory for AWS Auth configuration
    /// Can be set via AWS_AUTH_CONFIG_DIR environment variable
    /// Default: ~/.aws-auth
    #[arg(short = ARG_SHORT_CONFIG_DIR, long, env = "AWS_AUTH_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// Force new credential retrieval instead of using cached credentials
    /// Default: false (use cached credentials when available)
    #[arg(short = ARG_SHORT_IGNORE_CACHE, long, default_value_t = false)]
    pub ignore_cache: bool,

    /// Suppress status and progress messages
    /// Default: false (show operational logs)
    #[arg(short = 's', long, default_value_t = false)]
    pub silent: bool,
}

/// Batch commands for operations across multiple AWS accounts
#[derive(Subcommand)]
pub enum Batch {
    /// Execute a command across multiple AWS accounts
    ///
    /// Runs the specified command with appropriate credentials for each
    /// account that matches the filtering criteria.
    Exec {
        #[clap(flatten)]
        batch_common: BatchCommonArgs,

        /// Hide command output during execution
        /// Default: false (display command output)
        #[arg(short = 's', long, default_value_t = false)]
        suppress_output: bool,

        /// Directory to save per-account output files
        /// No effect if suppress_output is enabled
        #[arg(short = 'o', long)]
        output_dir: Option<PathBuf>,

        /// Command and arguments to execute
        /// Must be provided after -- separator
        /// Example: aws-auth batch exec -A prod-account -- aws s3 ls
        #[arg(trailing_var_arg = true)]
        arguments: Vec<String>,
    },
}
impl Batch {
    pub fn get_common_args(&self) -> &BatchCommonArgs {
        match self {
            Batch::Exec { batch_common, .. } => batch_common,
        }
    }
}
