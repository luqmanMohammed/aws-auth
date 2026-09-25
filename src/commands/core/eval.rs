use aws_config::Region;

use jiff::Timestamp;

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
                "expiration": expiration(&credentials)
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
            println!("{prefix}AWS_REGION={quote}{}{quote}", exec_inputs.region);
            println!(
                "{prefix}AWS_DEFAULT_REGION={quote}{}{quote}",
                exec_inputs.region
            );
            if let Some(expiry) = expiration(&credentials) {
                println!("{prefix}AWS_SSO_SESSION_EXPIRATION={quote}{expiry}{quote}");
            }
        }
    }
}

fn expiration(credentials: &Credentials) -> Option<String> {
    let expiry = Timestamp::try_from(credentials.expiry()?).ok()?;
    Some(format!("{expiry:.0}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn the_expiration_is_rfc3339_utc_truncated_to_whole_seconds() {
        let expiry = UNIX_EPOCH + Duration::new(1_800_000_000, 987_654_321);
        let credentials = Credentials::new("id", "secret", None, Some(expiry), "test");

        assert_eq!(
            expiration(&credentials).as_deref(),
            Some("2027-01-15T08:00:00Z")
        );
    }
}
