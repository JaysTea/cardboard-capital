//! Makes developer keys for Cardboard Capital. Dev-only: `cargo build --release` never builds it,
//! and nothing secret is needed to compile it.
//!
//!   cargo run --example devkey -- gen [private-key path]
//!       Makes a signing key (default %APPDATA%\cardboard-capital-dev\signing.pk8) and prints the
//!       line to add to DEV_PUBLIC_KEYS in src/account.rs.
//!   cargo run --example devkey -- sign <name> [--key <path>] [--id k1] [--out <path>]
//!       Writes a developer.key (default Documents\Cardboard Capital\account\developer.key).
//!
//! Keep the private key out of the repo and backed up offline: anyone with it can make keys.

#[path = "../src/account.rs"]
#[allow(dead_code)]
mod account;

use std::path::{Path, PathBuf};

use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

const USAGE: &str = "usage:\n  devkey gen [private-key path]\n  devkey sign <name> [--key <path>] [--id k1] [--out <path>]";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("gen") => gen(args.get(1).map(PathBuf::from).unwrap_or_else(default_key)),
        Some("sign") => sign(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn default_key() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cardboard-capital-dev")
        .join("signing.pk8")
}

/// Refuses paths inside this repo, so the private key can't end up in a commit.
fn outside_repo(path: &Path) -> Result<(), String> {
    let repo = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).map_err(|e| e.to_string())?;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let dir = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
    if dir.starts_with(&repo) {
        return Err(format!("{} is inside the repo; keep the private key somewhere else.", path.display()));
    }
    Ok(())
}

fn gen(path: PathBuf) -> Result<(), String> {
    outside_repo(&path)?;
    if path.exists() {
        return Err(format!(
            "{} already exists. Every key it signed depends on it; delete it yourself if you really want a new one.",
            path.display()
        ));
    }
    let pk8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| "No random numbers from the OS")?;
    std::fs::write(&path, pk8.as_ref()).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    let kp = Ed25519KeyPair::from_pkcs8(pk8.as_ref()).map_err(|e| e.to_string())?;
    println!("Private key written to {} (back it up offline).", path.display());
    println!("Add this to DEV_PUBLIC_KEYS in src/account.rs (pick a new id, e.g. k2, when rotating):\n");
    println!("    (\"k1\", {}),", rust_bytes(kp.public_key().as_ref()));
    Ok(())
}

fn sign(args: &[String]) -> Result<(), String> {
    let mut name = None;
    let mut key = default_key();
    let mut id = "k1".to_string();
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--key" => key = PathBuf::from(value()?),
            "--id" => id = value()?,
            "--out" => out = Some(PathBuf::from(value()?)),
            _ if name.is_none() => name = Some(a.clone()),
            _ => return Err(format!("unexpected {a}\n{USAGE}")),
        }
    }
    let name = name.ok_or(USAGE)?;
    let out = out.unwrap_or_else(|| {
        dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Cardboard Capital")
            .join("account")
            .join(account::KEY_FILE)
    });

    let pk8 = std::fs::read(&key).map_err(|e| format!("Couldn't read the private key {}: {e}", key.display()))?;
    let kp = Ed25519KeyPair::from_pkcs8(&pk8).map_err(|e| format!("{} isn't a signing key: {e}", key.display()))?;
    let public: [u8; 32] = kp.public_key().as_ref().try_into().map_err(|_| "unexpected public key size")?;
    let issued = chrono::Local::now().format("%Y-%m-%d").to_string();
    let payload = account::payload(&id, &name, &issued)?;
    let text = format!("{payload}\n{}\n", account::to_hex(kp.sign(payload.as_bytes()).as_ref()));
    account::verify(&text, &[(id.as_str(), public)])?;

    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    }
    std::fs::write(&out, &text).map_err(|e| format!("Couldn't write {}: {e}", out.display()))?;
    println!("Developer key for {name} written to {}.", out.display());
    if !account::DEV_PUBLIC_KEYS.contains(&(id.as_str(), public)) {
        println!(
            "Note: this build's DEV_PUBLIC_KEYS doesn't list key {id} with this public key yet, so the app \
             won't accept it until you add:\n    (\"{id}\", {}),",
            rust_bytes(&public)
        );
    }
    Ok(())
}

/// `[0x01, 0x02, …]`
fn rust_bytes(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("0x{b:02x}")).collect();
    format!("[{}]", parts.join(", "))
}
