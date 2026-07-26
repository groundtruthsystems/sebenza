use crate::domain::config::AutoNameConfig;
use crate::domain::policies::{generate_fallback_branch_name, is_valid_branch_name};
use crate::services::llm_spawn::{llm_provider_label, run_short_llm_task, RunLlmResult};
use std::time::Duration;

const MAX_BRANCH_LENGTH: usize = 40;
const AUTO_NAME_TIMEOUT: Duration = Duration::from_secs(15);

fn default_system_prompt() -> String {
    [
        "Generate a concise git branch name from the task description.",
        "Return only the branch name.",
        "Use lowercase kebab-case.",
        &format!("Maximum {MAX_BRANCH_LENGTH} characters."),
        "Do not include quotes, code fences, or prefixes like feature/ or fix/.",
    ]
    .join(" ")
}

/// Clean an LLM's raw output into a valid branch name (port of
/// `normalizeGeneratedBranchName`).
fn normalize_generated_branch_name(raw: &str) -> Result<String, String> {
    let mut branch = raw.trim().to_string();
    // Strip a leading ```lang fence and trailing ``` fence.
    if let Some(rest) = branch.strip_prefix("```") {
        branch = rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '-').trim_start().to_string();
    }
    if let Some(rest) = branch.strip_suffix("```") {
        branch = rest.trim_end().to_string();
    }
    // First line only.
    branch = branch.lines().next().unwrap_or("").trim().to_string();
    // Drop a leading "branch:" / "branch name:" label (case-insensitive).
    let lower = branch.to_lowercase();
    for prefix in ["branch name:", "branch:"] {
        if lower.starts_with(prefix) {
            branch = branch[prefix.len()..].trim_start().to_string();
            break;
        }
    }
    // Strip surrounding quotes/backticks.
    branch = branch.trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string();
    branch = branch.to_lowercase();
    // Replace unsafe chars, collapse `/`/`.` and `-` runs, trim dashes.
    branch = replace_runs(&branch, |c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '/' | '-')), '-');
    branch = replace_runs(&branch, |c| c == '/' || c == '.', '-');
    branch = collapse_dashes(&branch);
    branch = branch.trim_matches('-').to_string();
    branch = branch.chars().take(MAX_BRANCH_LENGTH).collect::<String>().trim_end_matches('-').to_string();

    if branch.is_empty() {
        return Err("Auto-name model returned an empty branch name".to_string());
    }
    if !is_valid_branch_name(&branch) {
        return Err(format!("Auto-name model returned an invalid branch name: {branch}"));
    }
    Ok(branch)
}

/// Replace every maximal run of chars matching `bad` with a single `to`.
fn replace_runs(s: &str, bad: impl Fn(char) -> bool, to: char) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for ch in s.chars() {
        if bad(ch) {
            if !in_run {
                out.push(to);
            }
            in_run = true;
        } else {
            out.push(ch);
            in_run = false;
        }
    }
    out
}

fn collapse_dashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch == '-' {
            if !prev_dash {
                out.push(ch);
            }
            prev_dash = true;
        } else {
            out.push(ch);
            prev_dash = false;
        }
    }
    out
}

fn build_prompt(prompt: &str) -> String {
    format!(
        "Here is the task description: {prompt}. You MUST return the branch name only, no other text or comments. Be fast, make it simple, and concise."
    )
}

/// Generate a branch name from `task` via the configured LLM. Blocking.
pub fn generate_branch_name(config: &AutoNameConfig, task: &str) -> Result<String, String> {
    let prompt = task.trim();
    if prompt.is_empty() {
        return Err("Auto-name requires a prompt".to_string());
    }

    let system_prompt = config
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(default_system_prompt);
    let cli = llm_provider_label(config);

    match run_short_llm_task(config, &system_prompt, &build_prompt(prompt), AUTO_NAME_TIMEOUT) {
        RunLlmResult::Ok { stdout } => {
            let output = stdout.trim();
            if output.is_empty() {
                return Err(format!("{cli} returned empty output"));
            }
            normalize_generated_branch_name(output)
        }
        RunLlmResult::Timeout => {
            // A timeout falls back to a random name rather than failing.
            Ok(generate_fallback_branch_name())
        }
        RunLlmResult::SpawnError => {
            Err(format!("'{cli}' CLI not found. Install it or check your PATH."))
        }
        RunLlmResult::ExitNonzero { exit_code, stdout, stderr } => {
            let detail = {
                let s = stderr.trim();
                if !s.is_empty() {
                    s.to_string()
                } else if !stdout.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    format!("exit {exit_code}")
                }
            };
            Err(format!("{cli} failed: {detail}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fences_labels_and_normalizes() {
        assert_eq!(normalize_generated_branch_name("Add Feature X").unwrap(), "add-feature-x");
        assert_eq!(normalize_generated_branch_name("Branch: Fix/The Bug").unwrap(), "fix-the-bug");
        assert_eq!(normalize_generated_branch_name("```\nmy-branch\n```").unwrap(), "my-branch");
        assert_eq!(normalize_generated_branch_name("\"quoted-name\"").unwrap(), "quoted-name");
    }

    #[test]
    fn truncates_to_max_length() {
        let long = "a".repeat(60);
        let out = normalize_generated_branch_name(&long).unwrap();
        assert_eq!(out.chars().count(), MAX_BRANCH_LENGTH);
    }

    #[test]
    fn empty_after_normalization_errors() {
        assert!(normalize_generated_branch_name("///").is_err());
    }
}
