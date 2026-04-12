use crate::utils::toolmanager::{Compression, Semver, SystemPathCheck, ToolSpec, VersionMatch};
use std::env::consts;
use std::path::{PathBuf};

const SSM_PLUGIN_SPEC: ToolSpec = ToolSpec {
    name: "session-manager-plugin",
    expected_version: Semver(1, 2, 804),
    version_cmd: &["--version"],
    system_path_strategy: SystemPathCheck::Honor(VersionMatch::MajorMinor),
    compression: Some(Compression::Zip),
    download_url_builder: |version| {
        let platform = match consts::OS {
            "windows" => "windows",
            "macos" => {
                if consts::ARCH == "aarch64" {
                    "mac_arm64"
                } else {
                    "mac_x64"
                }
            }
            "linux" => {
                if consts::ARCH == "aarch64" {
                    "linux_arm64"
                } else {
                    "linux_x64"
                }
            }
            other => panic!("Unsupported OS: {other}"),
        };
        format!(
        "https://s3.amazonaws.com/session-manager-downloads/plugin/{}.0/{}/sessionmanager-bundle.zip", version, platform
    )
    },
};

pub struct SessionManager {
    tool_path: PathBuf,
    ssm_client: aws_sdk_ssm::Client,
}
