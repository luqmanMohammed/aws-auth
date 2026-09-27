use crate::utils::expiring::{Expiring, ExpiringFile};
use aws_sdk_sso::types::{AccountInfo, RoleInfo};
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const DIRECTORY_FILE_NAME: &str = "sso-directory.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingCache {
    Use,
    /// Lists again under the current sign-in.
    Refresh,
    /// Lists again after discarding the sign-in too, forcing a new device authorization.
    Ignore,
}

impl ListingCache {
    pub fn from_flags(ignore_cache: bool, refresh_list: bool) -> Self {
        match (ignore_cache, refresh_list) {
            (true, _) => Self::Ignore,
            (false, true) => Self::Refresh,
            (false, false) => Self::Use,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedAccount {
    account_id: Option<String>,
    account_name: Option<String>,
    email_address: Option<String>,
}

impl From<&AccountInfo> for CachedAccount {
    fn from(account: &AccountInfo) -> Self {
        Self {
            account_id: account.account_id.clone(),
            account_name: account.account_name.clone(),
            email_address: account.email_address.clone(),
        }
    }
}

impl From<CachedAccount> for AccountInfo {
    fn from(account: CachedAccount) -> Self {
        AccountInfo::builder()
            .set_account_id(account.account_id)
            .set_account_name(account.account_name)
            .set_email_address(account.email_address)
            .build()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedRole {
    account_id: Option<String>,
    role_name: Option<String>,
}

impl From<&RoleInfo> for CachedRole {
    fn from(role: &RoleInfo) -> Self {
        Self {
            account_id: role.account_id.clone(),
            role_name: role.role_name.clone(),
        }
    }
}

impl From<CachedRole> for RoleInfo {
    fn from(role: CachedRole) -> Self {
        RoleInfo::builder()
            .set_account_id(role.account_id)
            .set_role_name(role.role_name)
            .build()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct SsoDirectory {
    start_url: String,
    accounts: Option<Vec<CachedAccount>>,
    roles: HashMap<String, Vec<CachedRole>>,
}

impl SsoDirectory {
    fn new(start_url: &str) -> Self {
        Self {
            start_url: start_url.to_string(),
            accounts: None,
            roles: HashMap::new(),
        }
    }
}

/// Accounts and roles listed under one start URL, sharing a single expiry from the first
/// listing, so the whole directory is refetched together once it lapses.
pub struct DirectoryCache {
    file: ExpiringFile<SsoDirectory>,
    start_url: String,
    ttl: SignedDuration,
}

impl DirectoryCache {
    pub fn new(cache_dir: &Path, start_url: &str, ttl: SignedDuration) -> Self {
        Self {
            file: ExpiringFile::new(cache_dir.join(DIRECTORY_FILE_NAME)),
            start_url: start_url.to_string(),
            ttl,
        }
    }

    fn load(&self) -> Option<Expiring<SsoDirectory>> {
        match self.file.load() {
            Ok(directory) => {
                directory.filter(|directory| directory.data().start_url == self.start_url)
            }
            Err(err) => {
                eprintln!("WARN: Failed to read SSO directory cache: {err}");
                None
            }
        }
    }

    /// Merges into the live directory, keeping its expiry, unless `fresh` asks to start over.
    fn record(&self, fresh: bool, update: impl FnOnce(&mut SsoDirectory)) {
        let existing = if fresh { None } else { self.load() };
        let Some(mut directory) =
            existing.or_else(|| Expiring::new(SsoDirectory::new(&self.start_url), self.ttl))
        else {
            return;
        };
        update(directory.data_mut());
        if let Err(err) = self.file.store(&directory) {
            eprintln!("WARN: Failed to persist SSO directory cache: {err}");
        }
    }

    pub fn accounts(&self) -> Option<Vec<AccountInfo>> {
        let accounts = self.load()?.into_data().accounts?;
        Some(accounts.into_iter().map(AccountInfo::from).collect())
    }

    pub fn roles(&self, account_id: &str) -> Option<Vec<RoleInfo>> {
        let roles = self.load()?.into_data().roles.remove(account_id)?;
        Some(roles.into_iter().map(RoleInfo::from).collect())
    }

    pub fn record_accounts(&self, accounts: &[AccountInfo], fresh: bool) {
        self.record(fresh, |directory| {
            directory.accounts = Some(accounts.iter().map(CachedAccount::from).collect());
        });
    }

    pub fn record_roles(&self, account_id: &str, roles: &[RoleInfo], fresh: bool) {
        self.record(fresh, |directory| {
            directory.roles.insert(
                account_id.to_string(),
                roles.iter().map(CachedRole::from).collect(),
            );
        });
    }

    pub fn clear(&self) -> std::io::Result<()> {
        self.file.clear()
    }
}

// Tests were written by AI (Claude Opus 5.5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_support::TempDir;

    const START_URL: &str = "https://example.awsapps.com/start";

    fn cache(dir: &TempDir, start_url: &str) -> DirectoryCache {
        DirectoryCache::new(dir.path(), start_url, SignedDuration::from_hours(1))
    }

    fn on_disk(dir: &TempDir) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(dir.join(DIRECTORY_FILE_NAME)).unwrap()).unwrap()
    }

    fn account(id: &str) -> AccountInfo {
        AccountInfo::builder()
            .account_id(id)
            .account_name(format!("{id}-name"))
            .email_address(format!("{id}@example.com"))
            .build()
    }

    fn role(account_id: &str, name: &str) -> RoleInfo {
        RoleInfo::builder()
            .account_id(account_id)
            .role_name(name)
            .build()
    }

    #[test]
    fn ignoring_the_cache_outranks_refreshing_the_list() {
        assert_eq!(ListingCache::from_flags(false, false), ListingCache::Use);
        assert_eq!(ListingCache::from_flags(false, true), ListingCache::Refresh);
        assert_eq!(ListingCache::from_flags(true, false), ListingCache::Ignore);
        assert_eq!(ListingCache::from_flags(true, true), ListingCache::Ignore);
    }

    #[test]
    fn nothing_is_cached_until_recorded() {
        let dir = TempDir::new("directory-empty");
        assert!(cache(&dir, START_URL).accounts().is_none());
        assert!(cache(&dir, START_URL).roles("1").is_none());
    }

    #[test]
    fn recorded_accounts_and_roles_read_back_unchanged() {
        let dir = TempDir::new("directory-roundtrip");
        cache(&dir, START_URL).record_accounts(&[account("1"), account("2")], false);
        cache(&dir, START_URL).record_roles("1", &[role("1", "admin")], false);

        assert_eq!(
            cache(&dir, START_URL).accounts(),
            Some(vec![account("1"), account("2")])
        );
        assert_eq!(
            cache(&dir, START_URL).roles("1"),
            Some(vec![role("1", "admin")])
        );
        assert!(cache(&dir, START_URL).roles("2").is_none());
    }

    #[test]
    fn merging_keeps_the_expiry_of_the_first_listing() {
        let dir = TempDir::new("directory-merge");
        cache(&dir, START_URL).record_accounts(&[account("1")], false);
        let first = on_disk(&dir)["expires_at"].clone();

        DirectoryCache::new(dir.path(), START_URL, SignedDuration::from_hours(5)).record_roles(
            "1",
            &[role("1", "admin")],
            false,
        );

        assert_eq!(on_disk(&dir)["expires_at"], first);
        assert!(
            cache(&dir, START_URL).accounts().is_some(),
            "accounts are kept"
        );
    }

    #[test]
    fn a_fresh_recording_drops_everything_else() {
        let dir = TempDir::new("directory-fresh");
        cache(&dir, START_URL).record_accounts(&[account("1")], false);

        cache(&dir, START_URL).record_roles("1", &[role("1", "admin")], true);

        assert!(cache(&dir, START_URL).accounts().is_none());
        assert!(cache(&dir, START_URL).roles("1").is_some());
    }

    #[test]
    fn a_directory_for_another_start_url_is_a_miss_and_is_replaced() {
        let dir = TempDir::new("directory-other-url");
        cache(&dir, "https://other.awsapps.com/start").record_accounts(&[account("1")], false);

        assert!(cache(&dir, START_URL).accounts().is_none());

        cache(&dir, START_URL).record_roles("2", &[role("2", "admin")], false);
        assert!(
            cache(&dir, START_URL).accounts().is_none(),
            "the other start URL's accounts must not be merged in"
        );
    }

    #[test]
    fn clearing_forgets_everything() {
        let dir = TempDir::new("directory-clear");
        cache(&dir, START_URL).record_accounts(&[account("1")], false);

        cache(&dir, START_URL).clear().unwrap();

        assert!(cache(&dir, START_URL).accounts().is_none());
    }
}
