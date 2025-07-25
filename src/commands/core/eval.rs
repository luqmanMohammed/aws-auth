use aws_config::Region;

use chrono::{DateTime, Utc};

use aws_sdk_ssooidc::config::Credentials;

use crate::cmd::EvalOutputFormat;

pub struct ExecEvalInputs<'a> {
    pub region: Region,
    pub output: &'a EvalOutputFormat,
}

pub fn exec_eval(credentials: Credentials, exec_inputs: ExecEvalInputs) {
    match exec_inputs.output {
        EvalOutputFormat::Json => {
            let output = serde_json::json!({
                "access_key_id": credentials.access_key_id(),
                "secret_access_key": credentials.secret_access_key(),
                "region": exec_inputs.region.to_string(),
                "session_token": credentials.session_token(),
                "expiration": credentials.expiry().map(|e| {
                    let dt: DateTime<Utc> = e.into();
                    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                })
            });
            println!("{}", output)
        }
        EvalOutputFormat::Eval => {
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
            if let Some(expiry) = credentials.expiry() {
                let dt: DateTime<Utc> = expiry.into();
                println!(
                    "export AWS_SSO_SESSION_EXPIRATION='{}'",
                    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                );
            }
        }
    }
}
