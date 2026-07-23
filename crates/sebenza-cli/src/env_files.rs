//! Load `.env.local` / `.env` from the launch dir into the process environment.
//! Records which keys were added so `serve` can tell the backend to keep the
//! launch project's secrets out of the tmux server's global environment.

use std::collections::BTreeSet;
use std::path::Path;

/// Load a single `.env` file, setting keys that aren't already present.
/// Returns the keys it added.
fn load_env_file(path: &Path, added: &mut BTreeSet<String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq].trim().to_string();
        let raw_val = trimmed[eq + 1..].trim();
        let val = raw_val
            .strip_prefix(['"', '\''])
            .and_then(|v| v.strip_suffix(['"', '\'']))
            .unwrap_or(raw_val)
            .to_string();
        if key.is_empty() {
            continue;
        }
        if std::env::var_os(&key).is_none() {
            // SAFETY: single-threaded CLI startup, before any threads spawn.
            unsafe { std::env::set_var(&key, &val) };
            added.insert(key);
        }
    }
}

/// Load `.env.local` (higher priority) then `.env` from `cwd`. Returns the set
/// of keys added to the environment.
pub fn load_project_env(cwd: &str) -> BTreeSet<String> {
    let mut added = BTreeSet::new();
    load_env_file(&Path::new(cwd).join(".env.local"), &mut added);
    load_env_file(&Path::new(cwd).join(".env"), &mut added);
    added
}
