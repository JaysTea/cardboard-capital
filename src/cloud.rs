//! Online storage, planned on Supabase (Postgres). A placeholder for now: it only says whether
//! cloud storage is set up, it makes no network calls, and only developer accounts see it.
//!
//! The plan, so hooking it up is configuration rather than a rewrite:
//! - Supabase has no official Rust SDK, so use its HTTP APIs with the app's `ureq` client:
//!   Auth (`POST {url}/auth/v1/token?grant_type=password`, …) and PostgREST
//!   (`{url}/rest/v1/<table>`), sending `apikey: <anon key>` and
//!   `Authorization: Bearer <the user's access token>`.
//! - The schema draft is in `supabase/migrations/`. It's plain Postgres, so it moves to a paid
//!   plan, or to Neon/RDS, with a dump and restore; only the auth layer is Supabase-specific.
//! - The server decides who is a developer (`profiles.role`, set in the Supabase dashboard) and
//!   Row Level Security enforces it. The local developer key only shows the UI.
//! - Keep the session token in Windows Credential Manager, never in Documents.

use crate::account::Account;

/// The Supabase project this build talks to, set at build time with
/// `CARDBOARD_CAPITAL_SUPABASE_URL` and `CARDBOARD_CAPITAL_SUPABASE_ANON_KEY`. The anon key is
/// public by design (Row Level Security protects the data), so it's fine inside the app.
#[allow(dead_code)] // read once sync is built
pub struct CloudConfig {
    pub url: &'static str,
    pub anon_key: &'static str,
}

pub fn config() -> Option<CloudConfig> {
    match (
        option_env!("CARDBOARD_CAPITAL_SUPABASE_URL"),
        option_env!("CARDBOARD_CAPITAL_SUPABASE_ANON_KEY"),
    ) {
        (Some(url), Some(anon_key)) if !url.trim().is_empty() && !anon_key.trim().is_empty() => {
            Some(CloudConfig { url, anon_key })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloudStatus {
    /// Normal accounts: online accounts aren't open yet.
    Unavailable,
    /// A developer, but this build has no Supabase project set.
    NotConfigured,
    /// A developer, and a project to connect to (signing in comes later).
    Ready,
}

impl CloudStatus {
    pub fn describe(self) -> &'static str {
        match self {
            CloudStatus::Unavailable => "Online accounts and cloud sync are coming.",
            CloudStatus::NotConfigured => "Supabase isn't configured for this build yet.",
            CloudStatus::Ready => "Supabase is configured. Signing in and syncing come next.",
        }
    }
}

pub fn status(account: &Account) -> CloudStatus {
    status_for(account.is_developer(), config().is_some())
}

fn status_for(developer: bool, configured: bool) -> CloudStatus {
    match (developer, configured) {
        (false, _) => CloudStatus::Unavailable,
        (true, false) => CloudStatus::NotConfigured,
        (true, true) => CloudStatus::Ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_developers_get_cloud() {
        assert_eq!(status_for(false, false), CloudStatus::Unavailable);
        assert_eq!(status_for(false, true), CloudStatus::Unavailable);
        assert_eq!(status_for(true, false), CloudStatus::NotConfigured);
        assert_eq!(status_for(true, true), CloudStatus::Ready);
        assert_eq!(status(&Account::default()), CloudStatus::Unavailable);
    }
}
