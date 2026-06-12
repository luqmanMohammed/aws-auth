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
            #[cfg(windows)]
            let (prefix, quote) = ("$env:", '"');
            #[cfg(not(windows))]
            let (prefix, quote) = ("export ", '\'');

            println!(
                "{prefix}AWS_ACCESS_KEY_ID={quote}{}{quote}",
                credentials.access_key_id()
            );
            println!(
                "{prefix}AWS_SECRET_ACCESS_KEY={quote}{}{quote}",
                credentials.secret_access_key()
            );
            if let Some(token) = credentials.session_token() {
                println!("{prefix}AWS_SESSION_TOKEN={quote}{token}{quote}");
            }
            println!(
                "{prefix}AWS_REGION={quote}{}{quote}",
                exec_inputs.region
            );
            println!(
                "{prefix}AWS_DEFAULT_REGION={quote}{}{quote}",
                exec_inputs.region
            );
            if let Some(expiry) = credentials.expiry() {
                let dt: DateTime<Utc> = expiry.into();
                println!(
                    "{prefix}AWS_SSO_SESSION_EXPIRATION={quote}{}{quote}",
                    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                );
            }
        }
    }
}
