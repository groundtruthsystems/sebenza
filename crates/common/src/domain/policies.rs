use std::collections::BTreeSet;

/// Collapse runs of whitespace into a single `-`.
fn whitespace_to_dash(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                out.push('-');
            }
            in_ws = true;
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out
}

/// Collapse runs of 2+ `target` into a single `target`.
fn collapse_char(s: &str, target: char) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev = false;
    for ch in s.chars() {
        if ch == target {
            if !prev {
                out.push(ch);
            }
            prev = true;
        } else {
            out.push(ch);
            prev = false;
        }
    }
    out
}

/// Port of `sanitizeBranchName` (domain/policies.ts).
pub fn sanitize_branch_name(raw: &str) -> String {
    let mut s = whitespace_to_dash(&raw.to_lowercase());
    s.retain(|c| !matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | ']' | '\\'));
    s = s.replace("@{", "");
    s = collapse_char(&s, '.');
    s = collapse_char(&s, '/');
    s = collapse_char(&s, '-');
    s = s
        .trim_matches(|c| matches!(c, '.' | '-' | '/'))
        .to_string();
    if s.to_lowercase().ends_with(".lock") {
        s.truncate(s.len() - ".lock".len());
    }
    s
}

/// A branch name is valid iff it is non-empty and already in sanitized form.
pub fn is_valid_branch_name(raw: &str) -> bool {
    !raw.is_empty() && sanitize_branch_name(raw) == raw
}

/// Branch names offerable as the target of a *new* worktree: valid, sorted,
/// deduped, and excluding anything already checked out somewhere.
///
/// Because `checked_out` includes the main checkout's branch, the main branch is
/// (correctly) never offered here — it already has a worktree, the repo root.
pub fn available_branch_names(
    local: &[String],
    remote: &[String],
    checked_out: &BTreeSet<String>,
    include_remote: bool,
) -> Vec<String> {
    let mut names: BTreeSet<&str> = local
        .iter()
        .map(String::as_str)
        .filter(|b| is_valid_branch_name(b))
        .collect();
    if include_remote {
        names.extend(remote.iter().map(String::as_str).filter(|b| is_valid_branch_name(b)));
    }
    names
        .into_iter()
        .filter(|b| !checked_out.contains(*b))
        .map(str::to_string)
        .collect()
}

/// Branch names offerable as the *base* of a new worktree: valid, sorted,
/// deduped. Unlike `available_branch_names` this keeps checked-out branches, so
/// the main branch is offered — branching off it is the normal case.
pub fn base_branch_names(local: &[String]) -> Vec<String> {
    let names: BTreeSet<&str> = local
        .iter()
        .map(String::as_str)
        .filter(|b| is_valid_branch_name(b))
        .collect();
    names.into_iter().map(str::to_string).collect()
}

/// Valid env-var key: `^[a-z_][a-z0-9_]*$` (case-insensitive).
pub fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Allocate the next free service-port slot across existing worktree metas.
/// The first service with a
/// `portStart` is the reference for slot occupancy; all allocatable services get
/// `start + slot * step`.
pub fn allocate_service_ports(
    existing_metas: &[crate::domain::model::WorktreeMeta],
    services: &[crate::domain::config::ServiceSpec],
) -> std::collections::HashMap<String, u16> {
    let allocatable: Vec<&crate::domain::config::ServiceSpec> =
        services.iter().filter(|s| s.port_start.is_some()).collect();
    if allocatable.is_empty() {
        return std::collections::HashMap::new();
    }

    let reference = allocatable[0];
    let reference_start = reference.port_start.unwrap();
    let reference_step = reference.port_step.unwrap_or(1).max(1);
    let mut occupied: std::collections::HashSet<u16> = std::collections::HashSet::new();

    for meta in existing_metas {
        let Some(&port) = meta.allocated_ports.get(&reference.port_env) else {
            continue;
        };
        if port < reference_start {
            continue;
        }
        let diff = port - reference_start;
        if !diff.is_multiple_of(reference_step) {
            continue;
        }
        occupied.insert(diff / reference_step);
    }

    let mut slot: u16 = 1;
    while occupied.contains(&slot) {
        slot += 1;
    }

    let mut result = std::collections::HashMap::new();
    for service in allocatable {
        let start = service.port_start.unwrap();
        let step = service.port_step.unwrap_or(1);
        result.insert(service.port_env.clone(), start + slot * step);
    }
    result
}

/// `change-<8 hex>` fallback branch name.
pub fn generate_fallback_branch_name() -> String {
    format!("change-{}", crate::util::id::random_hex(4))
}

/// Path segments owned by the server's hub routes — a project prefix must not
/// collide with these or `/<prefix>` would shadow them.
const RESERVED_PROJECT_PREFIXES: [&str; 4] = ["api", "ws", "assets", "registry"];

/// Slug a string into a URL-path-friendly prefix (lowercase, hyphenated,
/// alphanumeric only). Empty if nothing usable remains.
pub fn sanitize_project_prefix(raw: &str) -> String {
    let lowered = raw.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Derive a Sebenza project URL prefix from a project dir's basename, adding
/// `-2`, `-3`, … to avoid collisions with taken prefixes and reserved segments.
pub fn derive_project_prefix<'a>(
    project_dir: &str,
    taken_prefixes: impl IntoIterator<Item = &'a str>,
) -> String {
    let basename = project_dir
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("sebenza");
    let base = {
        let s = sanitize_project_prefix(basename);
        if s.is_empty() { "sebenza".to_string() } else { s }
    };

    let mut taken: std::collections::HashSet<String> =
        taken_prefixes.into_iter().map(str::to_string).collect();
    taken.extend(RESERVED_PROJECT_PREFIXES.iter().map(|s| s.to_string()));

    if !taken.contains(&base) {
        return base;
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", crate::util::id::random_hex(4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_and_nested_branches() {
        assert!(is_valid_branch_name("main"));
        assert!(is_valid_branch_name("feature/foo-bar"));
        assert!(is_valid_branch_name("release/1.2.3"));
    }

    #[test]
    fn rejects_unsanitized() {
        assert!(!is_valid_branch_name(""));
        assert!(!is_valid_branch_name("Feature/Foo")); // uppercase
        assert!(!is_valid_branch_name("has space"));
        assert!(!is_valid_branch_name("bad~char"));
        assert!(!is_valid_branch_name("-leading"));
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn main_is_excluded_from_available_branches_because_the_root_holds_it() {
        // The repo root is a worktree checked out on `main`, so `main` is already
        // occupied and must not be offered for a NEW worktree. This is why the
        // checked-out set must NOT filter out the repo root — if someone
        // "helpfully" applies the root filter there, `main` leaks back in here
        // and creating a worktree on it fails later with a confusing error.
        let checked_out = crate::adapters::git::checked_out_branch_names(
            &crate::adapters::git::parse_git_worktree_porcelain(
                "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
                 worktree /repo/wt/feat-a\nHEAD def\nbranch refs/heads/feat-a\n",
            ),
        );
        let available = available_branch_names(
            &strings(&["main", "feat-a", "other-local"]),
            &[],
            &checked_out,
            false,
        );
        assert_eq!(available, strings(&["other-local"]));
    }

    #[test]
    fn base_branches_still_offer_main() {
        // Branching off main is the normal case, so the base list keeps it even
        // though it is checked out in the repo root.
        assert_eq!(
            base_branch_names(&strings(&["main", "feat-a"])),
            strings(&["feat-a", "main"])
        );
    }

    #[test]
    fn available_branches_are_sorted_deduped_and_validated() {
        let available = available_branch_names(
            &strings(&["zeta", "alpha", "alpha", "Bad/Name"]),
            &[],
            &BTreeSet::new(),
            false,
        );
        assert_eq!(available, strings(&["alpha", "zeta"]));
    }

    #[test]
    fn remote_branches_are_included_only_when_requested() {
        let local = strings(&["alpha"]);
        let remote = strings(&["origin/beta"]);
        assert_eq!(
            available_branch_names(&local, &remote, &BTreeSet::new(), false),
            strings(&["alpha"])
        );
        assert_eq!(
            available_branch_names(&local, &remote, &BTreeSet::new(), true),
            strings(&["alpha", "origin/beta"])
        );
    }

    /// A repo whose basename matches a hub route must not be able to shadow it
    /// — `/registry` serves the portfolio, so it is reserved alongside api/ws.
    #[test]
    fn project_prefixes_never_shadow_hub_routes() {
        for reserved in ["api", "ws", "assets", "registry"] {
            let prefix = derive_project_prefix(&format!("/home/dev/{reserved}"), []);
            assert_eq!(prefix, format!("{reserved}-2"), "{reserved} must be reserved");
        }
        assert_eq!(derive_project_prefix("/home/dev/my-app", []), "my-app");
    }
}
