use std::num::ParseIntError;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SemverError {
    #[error("missing version segment")]
    MissingSegment,
    #[error("invalid version segment: {0}")]
    InvalidSegment(#[from] ParseIntError),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Semver(pub u64, pub u64, pub u64);

impl FromStr for Semver {
    type Err = SemverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.splitn(3, '.');
        let major = parts.next().ok_or(SemverError::MissingSegment)?.parse()?;
        let minor = parts.next().ok_or(SemverError::MissingSegment)?.parse()?;
        let patch = parts.next().ok_or(SemverError::MissingSegment)?.parse()?;
        Ok(Semver(major, minor, patch))
    }
}

impl std::fmt::Display for Semver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

pub enum SystemPathCheck {
    Ignore,
    Honor(VersionMatch),
}

pub enum VersionMatch {
    MajorOnly,
    MajorMinor,
    Exact,
    Any,
}

pub enum Compression {
    Zip,
}

pub type DownloadUrlBuilder = fn(&Semver) -> String;

pub struct ToolSpec<'a> {
    pub name: &'a str,
    pub expected_version: Semver,
    pub download_url_builder: DownloadUrlBuilder,
    pub version_cmd: &'a [&'a str],
    pub system_path_strategy: SystemPathCheck,
    pub compression: Option<Compression>,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid version: {0}")]
    SemverParseError(#[from] SemverError),
    #[error("failed to resolve version for tool: {0}")]
    SemverResolutionFailed(String),
}

pub fn ensure_tool<'a>(config_dir: &Path, spec: &ToolSpec<'a>) -> Result<PathBuf, ToolError> {
    if let SystemPathCheck::Honor(ref vm) = spec.system_path_strategy {}
    let tool_path = config_dir.join("bin").join(&spec.name);

    todo!()
}
