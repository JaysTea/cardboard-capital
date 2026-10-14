//! Accounts. Besides a normal account there's a developer account, unlocked by a signed key
//! file, `account\developer.key` in the data folder (made with `cargo run --example devkey`).
//!
//! This check runs in the app only, and the app is open source: anyone can rebuild it without
//! the check. So developer mode only reveals developer screens and diagnostics. It never changes
//! where data lives or the update channel, and never unlocks anything destructive. Anything that
//! needs real protection (the planned online storage) must be enforced by the server, which
//! decides on its own who is a developer (see `cloud.rs`).
//!
//! Also included by `examples/devkey.rs`, so it must not use anything else from the app.

use std::io::Read;
use std::path::Path;

use ring::signature::{UnparsedPublicKey, ED25519};

pub const KEY_FILE: &str = "developer.key";
/// Start of a developer key's first line. The signature covers exactly that line, so with this
/// prefix it can't be valid for anything else.
pub const PAYLOAD_PREFIX: &str = "cardboard-capital/developer/v1;";

/// Public keys that sign developer keys, by id (`key=` in the payload). Add one to rotate;
/// remove one to revoke every key it signed. Printed by `cargo run --example devkey -- gen`.
pub const DEV_PUBLIC_KEYS: &[(&str, [u8; 32])] = &[(
    "k1",
    [
        0xc4, 0xc5, 0x4e, 0x30, 0xdf, 0xd0, 0x43, 0xd1, 0xc6, 0x36, 0x5a, 0xfd, 0x3f, 0x40, 0xa2, 0x99, 0xf2, 0x26,
        0x07, 0x29, 0xbd, 0x1d, 0xb8, 0x87, 0xe2, 0x37, 0x94, 0xfd, 0xc7, 0x6e, 0x02, 0xe0,
    ],
)];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AccountKind {
    #[default]
    Normal,
    Developer,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub kind: AccountKind,
    pub name: String,
    pub key_id: String,
    pub issued: String,
    /// Why a developer.key that's there wasn't accepted.
    pub problem: Option<String>,
}

impl Account {
    pub fn is_developer(&self) -> bool {
        self.kind == AccountKind::Developer
    }
}

/// A verified developer key.
#[derive(Debug, PartialEq)]
pub struct DevKey {
    pub id: String,
    pub name: String,
    pub issued: String,
}

pub fn load(account_dir: &Path) -> Account {
    load_with(account_dir, DEV_PUBLIC_KEYS)
}

fn load_with(account_dir: &Path, keys: &[(&str, [u8; 32])]) -> Account {
    let path = account_dir.join(KEY_FILE);
    let problem = |msg: String| Account { problem: Some(msg), ..Default::default() };
    let mut text = String::new();
    match std::fs::File::open(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Account::default(),
        Err(e) => return problem(format!("Couldn't read {}: {e}", path.display())),
        // A real key is a few hundred bytes.
        Ok(f) => {
            if let Err(e) = f.take(4096).read_to_string(&mut text) {
                return problem(format!("Couldn't read {}: {e}", path.display()));
            }
        }
    }
    match verify(&text, keys) {
        Ok(key) => Account {
            kind: AccountKind::Developer,
            name: key.name,
            key_id: key.id,
            issued: key.issued,
            problem: None,
        },
        Err(e) => problem(format!("{} isn't a valid developer key: {e}.", path.display())),
    }
}

/// The line a developer key signs. Names can't contain `;` or line breaks.
#[allow(dead_code)] // used by examples/devkey.rs
pub fn payload(id: &str, name: &str, issued: &str) -> Result<String, String> {
    if [id, name, issued].iter().any(|s| s.contains([';', '\r', '\n']) || s.trim().is_empty()) {
        return Err("id, name and date must be non-empty and can't contain ';' or line breaks".into());
    }
    Ok(format!("{PAYLOAD_PREFIX}key={id};name={name};issued={issued}"))
}

/// Checks a key file's text: the payload line, then its Ed25519 signature in hex.
pub fn verify(text: &str, keys: &[(&str, [u8; 32])]) -> Result<DevKey, String> {
    let mut lines = text.lines();
    let payload = lines.next().unwrap_or_default();
    let fields = payload
        .strip_prefix(PAYLOAD_PREFIX)
        .ok_or("it isn't a Cardboard Capital developer key")?;
    let sig_hex = lines.next().ok_or("it has no signature line")?.trim();
    let (mut id, mut name, mut issued) = (None, None, None);
    for part in fields.split(';') {
        match part.split_once('=') {
            Some(("key", v)) => id = Some(v),
            Some(("name", v)) => name = Some(v),
            Some(("issued", v)) => issued = Some(v),
            _ => {}
        }
    }
    let id = id.ok_or("it doesn't say which key signed it")?;
    let public = keys
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, pk)| pk)
        .ok_or_else(|| format!("it was signed with an unknown or revoked key ({id})"))?;
    let sig = from_hex::<64>(sig_hex).ok_or("its signature isn't 128 hex characters")?;
    UnparsedPublicKey::new(&ED25519, public)
        .verify(payload.as_bytes(), &sig)
        .map_err(|_| "its signature doesn't match")?;
    Ok(DevKey {
        id: id.to_string(),
        name: name.unwrap_or_default().to_string(),
        issued: issued.unwrap_or_default().to_string(),
    })
}

#[allow(dead_code)] // used by examples/devkey.rs
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.as_bytes();
    if s.len() != N * 2 {
        return None;
    }
    let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; N];
    for (i, pair) in s.as_chunks::<2>().0.iter().enumerate() {
        out[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn keypair(seed: u8) -> (Ed25519KeyPair, [u8; 32]) {
        let kp = Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap();
        let public: [u8; 32] = kp.public_key().as_ref().try_into().unwrap();
        (kp, public)
    }

    fn key_file(kp: &Ed25519KeyPair, payload: &str) -> String {
        format!("{payload}\r\n{}\r\n", to_hex(kp.sign(payload.as_bytes()).as_ref()))
    }

    #[test]
    fn valid_key_is_a_developer() {
        let (kp, public) = keypair(7);
        let text = key_file(&kp, &payload("t1", "JaysTea", "2026-10-14").unwrap());
        let key = verify(&text, &[("t1", public)]).unwrap();
        assert_eq!(key, DevKey { id: "t1".into(), name: "JaysTea".into(), issued: "2026-10-14".into() });
    }

    #[test]
    fn forged_or_broken_keys_are_rejected() {
        let (kp, public) = keypair(7);
        let (_, other) = keypair(9);
        let good = key_file(&kp, &payload("t1", "JaysTea", "2026-10-14").unwrap());
        let keys = [("t1", public)];
        // Someone edits the name.
        assert!(verify(&good.replace("JaysTea", "Mallory"), &keys).is_err());
        // Signed by a key the app doesn't know, or one that was revoked.
        assert!(verify(&good, &[("t2", public)]).is_err());
        assert!(verify(&good, &[("t1", other)]).is_err());
        assert!(verify(&good, &[]).is_err());
        // Damaged files.
        assert!(verify("", &keys).is_err());
        assert!(verify(good.lines().next().unwrap(), &keys).is_err());
        assert!(verify(&good.replace('a', "z"), &keys).is_err());
        assert!(verify("hello\nworld", &keys).is_err());
        assert!(payload("t1", "a;b", "x").is_err());
    }

    #[test]
    fn missing_file_is_a_normal_account() {
        let dir = std::env::temp_dir().join(format!("cardboard_capital_account_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (kp, public) = keypair(7);
        assert_eq!(load_with(&dir, &[("t1", public)]), Account::default());

        std::fs::write(dir.join(KEY_FILE), key_file(&kp, &payload("t1", "JaysTea", "2026-10-14").unwrap())).unwrap();
        let acc = load_with(&dir, &[("t1", public)]);
        assert!(acc.is_developer() && acc.problem.is_none());
        assert_eq!(acc.name, "JaysTea");

        std::fs::write(dir.join(KEY_FILE), "nonsense").unwrap();
        let acc = load_with(&dir, &[("t1", public)]);
        assert!(!acc.is_developer() && acc.problem.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(to_hex(&[0, 15, 255]), "000fff");
        assert_eq!(from_hex::<3>("000fff"), Some([0, 15, 255]));
        assert_eq!(from_hex::<3>("000FFF"), Some([0, 15, 255]));
        assert_eq!(from_hex::<3>("000ff"), None);
        assert_eq!(from_hex::<3>("000ffg"), None);
    }
}
