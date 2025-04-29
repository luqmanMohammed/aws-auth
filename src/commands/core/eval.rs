use aws_config::Region;

use chrono::{DateTime, Utc};

use aws_sdk_ssooidc::config::Credentials;

pub struct ExecEvalInputs {
    pub region: Region,
}

pub fn exec_eval(credentials: Credentials, exec_inputs: ExecEvalInputs) {
    println!("export AWS_ACCESS_KEY_ID='{}'", credentials.access_key_id());
    println!(
        "export AWS_SECRET_ACCESS_KEY='{}'",
        credentials.secret_access_key()
    );
    if credentials.session_token().is_some() {
        println!(
            "export AWS_SESSION_TOKEN='{}'",
            credentials.session_token().unwrap_or_default()
        );
    }
    println!("export AWS_REGION='{}'", exec_inputs.region);
    println!("export AWS_DEFAULT_REGION='{}'", exec_inputs.region);
    if credentials.expiry().is_some() {
        let dt: DateTime<Utc> = credentials.expiry().unwrap().into();
        println!(
            "export AWS_SSO_SESSION_EXPIRATION='{}'",
            dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
    }
}
