use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// AWS Account ID to authenticate against.
    #[arg(short, long)]
    pub account_id: String,

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
}

#[derive(Subcommand)]
pub enum Commands {
    /// The `Eks` subcommand is used to print a valid Kubernetes authentication object
    /// to be used with the Kubernetes external authentication process.
    /// This is particularly useful when authenticating with an AWS EKS cluster.
    Eks {
        /// The name of the EKS cluster for which to generate the authentication object.
        #[arg(short, long)]
        cluster: String,

        /// The AWS region where the specified EKS cluster is located.
        #[arg(short, long)]
        region: String,

        /// Optional cache directory for storing EKS authentication tokens.
        /// If not specified, a default cache location is used.
        #[arg(short, long)]
        eks_cache_dir: Option<PathBuf>,

        #[arg(short = 's', long)]
        eks_expiry_seconds: Option<usize>,
    },

    /// The `Eval` subcommand is used to print AWS environment variables.
    /// These variables can be used in shell `eval` commands to set up
    /// the AWS environment for subsequent commands or scripts.
    Eval {
        /// The AWS region to export as default and selected region
        #[arg(short, long, default_value_t=String::from("eu-west-2"))]
        region: String,
    },
}
