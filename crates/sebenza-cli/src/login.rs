//! `sebenza-cli login`: get an operator token, confirmed by the operator in
//! the dashboard (FR-10, TS-51).
//!
//! The CLI starts a login with the control token and the SHA-256 of a secret
//! verifier, prints the short code, and polls. The operator matches the code
//! in the dashboard and confirms; the CLI then collects the token (once) and
//! caches it at `~/.config/sebenza/operator-token.json`, mode 0600, with its
//! expiry. An expired cache is ignored.

use crate::http::Http;
use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CachedToken {
    pub token: String,
    /// Unix seconds.
    pub expires_at: u64,
}

pub fn operator_token_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("sebenza")
        .join("operator-token.json")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write the token owner-only, temp-then-rename, so it is never briefly
/// readable by others.
pub fn save_cached(path: &Path, cached: &CachedToken) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("cannot write {}", tmp.display()))?;
    f.write_all(serde_json::to_string(cached)?.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// The cached token, if present and not expired at `now`.
pub fn load_cached(path: &Path, now: u64) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let cached: CachedToken = serde_json::from_str(&text).ok()?;
    (cached.expires_at > now).then_some(cached.token)
}

/// The operator token for operator-only commands, or a hint to log in.
pub fn operator_token() -> Result<String> {
    load_cached(&operator_token_path(), unix_now())
        .ok_or_else(|| anyhow!("not signed in as the operator: run `sebenza-cli login`"))
}

pub async fn run(args: &[String], port: u16) -> i32 {
    let path = operator_token_path();
    if args.iter().any(|a| a == "--logout") {
        let _ = std::fs::remove_file(&path);
        println!("Signed out: removed {}", path.display());
        return 0;
    }
    if args.iter().any(|a| a == "--status") {
        return match load_cached(&path, unix_now()) {
            Some(_) => {
                println!("Signed in as the operator.");
                0
            }
            None => {
                println!("Not signed in. Run `sebenza-cli login`.");
                1
            }
        };
    }
    match login(&Http::new(port), &path).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("login failed: {e}");
            1
        }
    }
}

async fn login(http: &Http, path: &Path) -> Result<()> {
    let verifier = common::util::id::random_hex(32);
    let started = http
        .operator_login_start(&common::domain::solution::sha256_hex(&verifier))
        .await?;
    let id = started["loginId"]
        .as_str()
        .ok_or_else(|| anyhow!("server returned no login id"))?
        .to_string();
    let code = started["code"].as_str().unwrap_or("?");
    let expires_in = started["expiresIn"].as_u64().unwrap_or(300);
    println!("Confirm code {code} in the Sebenza dashboard to sign in as the operator.");
    println!("Waiting up to {} minutes…", expires_in.div_ceil(60));
    let deadline = std::time::Instant::now() + Duration::from_secs(expires_in + 5);
    while std::time::Instant::now() < deadline {
        let poll = http.operator_login_poll(&id, &verifier).await?;
        match poll["status"].as_str() {
            Some("confirmed") => {
                let cached = CachedToken {
                    token: poll["token"]
                        .as_str()
                        .ok_or_else(|| anyhow!("confirmed without a token"))?
                        .to_string(),
                    expires_at: poll["expiresAt"].as_u64().unwrap_or(0),
                };
                save_cached(path, &cached)?;
                println!("Signed in. Token cached at {} (0600).", path.display());
                return Ok(());
            }
            Some("denied") => return Err(anyhow!("the operator denied the sign-in")),
            Some("expired") => return Err(anyhow!("the sign-in expired before it was confirmed")),
            _ => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
    Err(anyhow!("the sign-in expired before it was confirmed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sebenza-cli-login-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("operator-token.json")
    }

    // TS-51 (file-mode unit): the cache is 0600 and honours its TTL.
    #[test]
    fn the_cache_is_owner_only_and_expires() {
        let path = temp("cache");
        let cached = CachedToken {
            token: "op_abc".into(),
            expires_at: 1_000,
        };
        save_cached(&path, &cached).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(load_cached(&path, 999).as_deref(), Some("op_abc"));
        assert_eq!(load_cached(&path, 1_000), None, "expired at its expiry");
        assert!(!path.with_extension("json.tmp").exists());

        // Overwriting keeps the mode.
        save_cached(
            &path,
            &CachedToken {
                token: "op_new".into(),
                expires_at: 2_000,
            },
        )
        .unwrap();
        assert_eq!(load_cached(&path, 1_500).as_deref(), Some("op_new"));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_missing_or_corrupt_cache_is_no_token() {
        let path = temp("corrupt");
        assert_eq!(load_cached(&path, 0), None);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_cached(&path, 0), None);
    }
}
