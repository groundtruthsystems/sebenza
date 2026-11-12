//! The operator credential (FR-10, FT-01, FT-14).
//!
//! Agents and the operator share one OS user and one control token, so a gate
//! the control token can open is no gate. Operator routes therefore accept
//! only an **operator token**, which:
//!
//! - lives in daemon memory only, minted per daemon run, with an 8 h TTL;
//! - reaches the SPA through a same-origin `POST /api/operator/session`;
//! - reaches `sebenza-cli` only through a login the operator confirms in the UI;
//! - is never put in an environment, a URL, a pane or a log line.
//!
//! The server derives the audit `caller` from which credential was presented
//! ([`CredentialClass`]), never from the self-declared `x-sebenza-caller`.
//!
//! Residual (known limitation): issuance rests on headers a same-uid process
//! can forge, so outside the lxc/Apple sandboxes the gates deter and audit
//! rather than enforce.

use common::domain::solution::CredentialClass;
use common::util::id::{random_hex, random_ulid};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Lifetime of an operator token (session or CLI).
pub const OPERATOR_TOKEN_TTL: Duration = Duration::from_secs(8 * 60 * 60);
/// How long a CLI login waits for the operator before it expires.
pub const LOGIN_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
struct Issued {
    token: String,
    expires: SystemTime,
}

#[derive(Clone)]
struct PendingLogin {
    code: String,
    client: String,
    verifier_hash: String,
    created: SystemTime,
    state: LoginState,
}

#[derive(Clone, PartialEq, Eq)]
enum LoginState {
    Pending,
    Denied,
    /// Confirmed; the CLI token waits here until the CLI collects it once.
    Confirmed(String),
}

/// A CLI login the operator can confirm or deny in the UI.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoginView {
    pub login_id: String,
    /// Shown by the CLI too, so the operator can match the two.
    pub code: String,
    pub client: String,
    pub created_at: u64,
}

/// What the CLI's poll sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginPoll {
    Pending,
    Denied,
    Expired,
    /// The token and its expiry (unix seconds). Returned once.
    Confirmed {
        token: String,
        expires_at: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoginError {
    #[error("no such login")]
    NotFound,
    #[error("the login verifier does not match")]
    BadVerifier,
    #[error("the login is no longer pending")]
    NotPending,
}

/// In-memory operator credentials for this daemon run.
pub struct OperatorAuth {
    session: Mutex<Option<Issued>>,
    cli: Mutex<Vec<Issued>>,
    logins: Mutex<HashMap<String, PendingLogin>>,
}

impl Default for OperatorAuth {
    fn default() -> Self {
        Self::new()
    }
}

fn unix(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_token() -> String {
    format!("op_{}", random_hex(32))
}

/// Constant-time-ish comparison; tokens are fixed-length hex.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

pub fn verifier_hash(verifier: &str) -> String {
    common::domain::solution::sha256_hex(verifier)
}

impl OperatorAuth {
    pub fn new() -> Self {
        Self {
            session: Mutex::new(None),
            cli: Mutex::new(Vec::new()),
            logins: Mutex::new(HashMap::new()),
        }
    }

    /// The SPA's session token: the current one, or a fresh one if none was
    /// issued this run or it expired. Returns the token and its expiry.
    pub fn issue_session_at(&self, now: SystemTime) -> (String, u64) {
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let current = s.as_ref().filter(|i| i.expires > now).cloned();
        let issued = current.unwrap_or_else(|| {
            let i = Issued {
                token: new_token(),
                expires: now + OPERATOR_TOKEN_TTL,
            };
            *s = Some(i.clone());
            i
        });
        (issued.token, unix(issued.expires))
    }

    pub fn issue_session(&self) -> (String, u64) {
        self.issue_session_at(SystemTime::now())
    }

    /// Which operator credential `token` is, if it is a live one.
    pub fn classify_at(&self, token: &str, now: SystemTime) -> Option<CredentialClass> {
        let s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if s.as_ref()
            .is_some_and(|i| i.expires > now && same(&i.token, token))
        {
            return Some(CredentialClass::OperatorSession);
        }
        drop(s);
        let mut cli = self.cli.lock().unwrap_or_else(|e| e.into_inner());
        cli.retain(|i| i.expires > now);
        cli.iter()
            .any(|i| same(&i.token, token))
            .then_some(CredentialClass::OperatorCli)
    }

    pub fn classify(&self, token: &str) -> Option<CredentialClass> {
        self.classify_at(token, SystemTime::now())
    }

    /// Start a CLI login. `verifier_hash` is the SHA-256 of a secret only the
    /// CLI holds, so only the CLI that started the login can collect it.
    /// Returns the login id and the short code both sides display.
    pub fn start_login_at(
        &self,
        client: &str,
        verifier_hash: &str,
        now: SystemTime,
    ) -> (String, String) {
        let id = random_ulid();
        let code = random_hex(3).to_ascii_uppercase();
        let mut logins = self.logins.lock().unwrap_or_else(|e| e.into_inner());
        logins.retain(|_, l| l.created + LOGIN_TTL > now);
        logins.insert(
            id.clone(),
            PendingLogin {
                code: code.clone(),
                client: client.chars().take(64).collect(),
                verifier_hash: verifier_hash.to_string(),
                created: now,
                state: LoginState::Pending,
            },
        );
        (id, code)
    }

    /// Logins waiting for the operator, oldest first.
    pub fn pending_logins_at(&self, now: SystemTime) -> Vec<LoginView> {
        let logins = self.logins.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<LoginView> = logins
            .iter()
            .filter(|(_, l)| l.state == LoginState::Pending && l.created + LOGIN_TTL > now)
            .map(|(id, l)| LoginView {
                login_id: id.clone(),
                code: l.code.clone(),
                client: l.client.clone(),
                created_at: unix(l.created),
            })
            .collect();
        out.sort_by(|a, b| a.login_id.cmp(&b.login_id));
        out
    }

    /// The operator confirms (`true`) or denies a pending login. Confirming
    /// mints a CLI token the CLI collects on its next poll.
    pub fn decide_login_at(
        &self,
        login_id: &str,
        confirm: bool,
        now: SystemTime,
    ) -> Result<(), LoginError> {
        let mut logins = self.logins.lock().unwrap_or_else(|e| e.into_inner());
        let l = logins.get_mut(login_id).ok_or(LoginError::NotFound)?;
        if l.state != LoginState::Pending || l.created + LOGIN_TTL <= now {
            return Err(LoginError::NotPending);
        }
        l.state = if confirm {
            let token = new_token();
            self.cli
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Issued {
                    token: token.clone(),
                    expires: now + OPERATOR_TOKEN_TTL,
                });
            LoginState::Confirmed(token)
        } else {
            LoginState::Denied
        };
        Ok(())
    }

    /// The CLI polls its login with the verifier it hashed at start. A
    /// confirmed token is handed over once, then the login is forgotten.
    pub fn poll_login_at(
        &self,
        login_id: &str,
        verifier: &str,
        now: SystemTime,
    ) -> Result<LoginPoll, LoginError> {
        let mut logins = self.logins.lock().unwrap_or_else(|e| e.into_inner());
        let l = logins.get(login_id).ok_or(LoginError::NotFound)?;
        if !same(&l.verifier_hash, &verifier_hash(verifier)) {
            return Err(LoginError::BadVerifier);
        }
        let expired = l.created + LOGIN_TTL <= now;
        match l.state.clone() {
            LoginState::Confirmed(token) => {
                logins.remove(login_id);
                let expires = self
                    .cli
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .find(|i| i.token == token)
                    .map(|i| unix(i.expires))
                    .unwrap_or(0);
                Ok(LoginPoll::Confirmed {
                    token,
                    expires_at: expires,
                })
            }
            LoginState::Denied => {
                logins.remove(login_id);
                Ok(LoginPoll::Denied)
            }
            LoginState::Pending if expired => {
                logins.remove(login_id);
                Ok(LoginPoll::Expired)
            }
            LoginState::Pending => Ok(LoginPoll::Pending),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    // TS-48 (token unit): a session token is stable within a run, expires
    // after its TTL, and a new daemon run never accepts the old one.
    #[test]
    fn session_tokens_live_for_one_run_and_one_ttl() {
        let auth = OperatorAuth::new();
        let (tok, exp) = auth.issue_session_at(t0());
        assert!(tok.starts_with("op_") && tok.len() == 3 + 64);
        assert_eq!(exp, unix(t0() + OPERATOR_TOKEN_TTL));
        assert_eq!(auth.issue_session_at(t0()).0, tok, "stable within the TTL");
        assert_eq!(
            auth.classify_at(&tok, t0()),
            Some(CredentialClass::OperatorSession)
        );
        let later = t0() + OPERATOR_TOKEN_TTL;
        assert_eq!(auth.classify_at(&tok, later), None, "expired");
        assert_ne!(
            auth.issue_session_at(later).0,
            tok,
            "re-minted after expiry"
        );

        let restarted = OperatorAuth::new();
        assert_eq!(
            restarted.classify_at(&tok, t0()),
            None,
            "a restart voids it"
        );
        assert_eq!(auth.classify_at("", t0()), None);
        assert_eq!(auth.classify_at("op_nope", t0()), None);
    }

    // TS-51 (login unit): confirmed logins yield a CLI token once; denied,
    // expired and unverified ones yield nothing.
    #[test]
    fn cli_logins_need_the_operator_and_the_verifier() {
        let auth = OperatorAuth::new();
        let (id, code) = auth.start_login_at("sebenza-cli", &verifier_hash("v1"), t0());
        assert_eq!(code.len(), 6);
        assert_eq!(auth.poll_login_at(&id, "v1", t0()), Ok(LoginPoll::Pending));
        assert_eq!(
            auth.poll_login_at(&id, "other", t0()),
            Err(LoginError::BadVerifier)
        );
        let pending = auth.pending_logins_at(t0());
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].code, code);

        auth.decide_login_at(&id, true, t0()).unwrap();
        assert!(auth.pending_logins_at(t0()).is_empty());
        let LoginPoll::Confirmed { token, expires_at } =
            auth.poll_login_at(&id, "v1", t0()).unwrap()
        else {
            panic!("confirmed");
        };
        assert_eq!(expires_at, unix(t0() + OPERATOR_TOKEN_TTL));
        assert_eq!(
            auth.classify_at(&token, t0()),
            Some(CredentialClass::OperatorCli)
        );
        assert_eq!(
            auth.poll_login_at(&id, "v1", t0()),
            Err(LoginError::NotFound),
            "handed over once"
        );
        assert_eq!(
            auth.classify_at(&token, t0() + OPERATOR_TOKEN_TTL),
            None,
            "the CLI token expires too"
        );

        let (denied, _) = auth.start_login_at("cli", &verifier_hash("v2"), t0());
        auth.decide_login_at(&denied, false, t0()).unwrap();
        assert_eq!(
            auth.poll_login_at(&denied, "v2", t0()),
            Ok(LoginPoll::Denied)
        );

        let (stale, _) = auth.start_login_at("cli", &verifier_hash("v3"), t0());
        let late = t0() + LOGIN_TTL;
        assert_eq!(
            auth.decide_login_at(&stale, true, late),
            Err(LoginError::NotPending),
            "an expired login cannot be confirmed"
        );
        assert_eq!(
            auth.poll_login_at(&stale, "v3", late),
            Ok(LoginPoll::Expired)
        );
        assert_eq!(
            auth.decide_login_at("nope", true, t0()),
            Err(LoginError::NotFound)
        );
    }

    // TS-48 (env half): the token is never placed in this process's
    // environment, so nothing the daemon spawns can inherit it.
    #[test]
    fn the_token_never_reaches_the_environment() {
        let auth = OperatorAuth::new();
        let (tok, _) = auth.issue_session();
        assert!(std::env::vars().all(|(_, v)| !v.contains(&tok)));
    }
}
