use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Args)]
pub struct CommonArgs {
    /// AWS Account ID to authenticate against.
    #[arg(short, long)]
    pub account: String,

    /// AWS IAM Role to assume during authentication.
    #[arg(short, long)]
    pub role: String,

    /// Optional cache directory for storing authentication tokens.
    /// If not provided, the default cache location will be used.
    #[arg(short = 'd', long)]
    pub cache_dir: Option<PathBuf>,

    /// Optional config path to retrieve AWS SSO Config.
    /// If not provided, the default config path will be used
    #[arg(short = 'o', long)]
    pub config_path: Option<PathBuf>,

    /// Flag to ignore the cache and request new credentials even if cached ones are available.
    /// Defaults to `false`.
    #[arg(short, long, default_value_t = false)]
    pub ignore_cache: bool,

    /// The AWS region to export as default and selected region.
    /// If not provided, it defaults to `eu-west-2`.
    #[arg(short='g', long, default_value_t=String::from("eu-west-2"))]
    pub region: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// The `Eks` subcommand is used to print a valid Kubernetes authentication object
    /// to be used with the Kubernetes external authentication process.
    /// This is particularly useful when authenticating with an AWS EKS cluster.
    Eks {
        #[clap(flatten)]
        common: CommonArgs,

        /// The name of the EKS cluster for which to generate the authentication object.
        #[arg(short, long)]
        cluster: String,

        /// Optional cache directory for storing EKS authentication tokens.
        /// If not specified, a default cache location is used.
        #[arg(short, long)]
        eks_cache_dir: Option<PathBuf>,

        /// Optional EKS auth token TTL in secounds.
        /// If not specified, default value of `900` secounds (15m) will be used.
        #[arg(short = 's', long)]
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
}
