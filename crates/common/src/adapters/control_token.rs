use crate::util::id::random_uuid;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

fn control_token_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".config").join("sebenza").join("control-token")
}

static CACHED_TOKEN: Mutex<Option<String>> = Mutex::new(None);

/// Load the cached control token, generating and persisting one on first use.
pub fn load_control_token() -> Result<String, String> {
    {
        let cached = CACHED_TOKEN.lock().unwrap();
        if let Some(token) = cached.as_ref() {
            return Ok(token.clone());
        }
    }

    let path = control_token_path();
    let token = if let Ok(existing) = fs::read_to_string(&path) {
        existing.trim().to_string()
    } else {
        let token = random_uuid();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&path, &token).map_err(|e| e.to_string())?;
        set_mode_600(&path);
        token
    };

    *CACHED_TOKEN.lock().unwrap() = Some(token.clone());
    Ok(token)
}

#[cfg(unix)]
fn set_mode_600(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_mode_600(_path: &std::path::Path) {}
