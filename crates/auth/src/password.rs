//! Argon2id password policy (Section 11.1).
//!
//! Hashes use PHC string format with the policy parameters below. `verify`
//! reports `RehashNeeded` when a stored hash was produced by a different
//! algorithm/version or weaker parameters, so a successful login can rehash
//! in place (legacy libsodium verification plugs into the same outcome during
//! the Phase 8 migration).

use argon2::{
    password_hash::{generate_salt, PasswordHasher, PasswordVerifier},
    Argon2, Params,
};
use std::sync::OnceLock;
use thiserror::Error;

/// Minimum accepted password length (characters, not bytes).
pub const PASSWORD_MIN_LEN: usize = 12;

/// Maximum accepted password length. Long inputs are rejected before hashing
/// so Argon2 memory cost cannot be weaponized against the host.
pub const PASSWORD_MAX_LEN: usize = 1024;

/// OWASP-recommended Argon2id baseline: 19 MiB memory, 2 iterations, parallelism 1.
const POLICY_M_COST_KIB: u32 = 19_456;
const POLICY_T_COST: u32 = 2;
const POLICY_P_COST: u32 = 1;
const POLICY_VERSION_DECIMAL: u32 = 19; // Argon2 v0x13

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PasswordError {
    #[error(
        "password length must be between {PASSWORD_MIN_LEN} and {PASSWORD_MAX_LEN} characters"
    )]
    LengthOutOfBounds,
    #[error("password hashing failed")]
    HashFailed,
}

/// Outcome of checking a presented password against a stored PHC hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Password matches the current policy parameters.
    Valid,
    /// Password does not match.
    Invalid,
    /// Password matches but the stored hash is below policy (legacy algorithm,
    /// version, or parameters); the caller must rehash on successful login.
    RehashNeeded,
}

fn policy_argon2() -> Argon2<'static> {
    // Params::new only fails on out-of-range constants; the panic-free path
    // falls back to library defaults rather than crashing the request thread.
    static POLICY: OnceLock<Argon2<'static>> = OnceLock::new();
    POLICY
        .get_or_init(|| {
            Params::new(POLICY_M_COST_KIB, POLICY_T_COST, POLICY_P_COST, None)
                .map(Argon2::from)
                .unwrap_or_default()
        })
        .clone()
}

/// Hashes a password with the policy parameters after the length check.
/// Returns the PHC-format string for storage.
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    validate_password_length(password)?;
    let salt = generate_salt();
    policy_argon2()
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| PasswordError::HashFailed)
}

/// Verifies a presented password against a stored PHC hash string.
///
/// A malformed stored hash yields [`VerifyOutcome::Invalid`] rather than an
/// error so a corrupted row cannot become an oracle distinguishing account
/// states.
pub fn verify_password(password: &str, stored_phc: &str) -> VerifyOutcome {
    let parsed = match argon2::password_hash::phc::PasswordHash::new(stored_phc) {
        Ok(parsed) => parsed,
        Err(_) => return VerifyOutcome::Invalid,
    };
    let matches = <Argon2 as PasswordVerifier<str>>::verify_password(
        &policy_argon2(),
        password.as_bytes(),
        stored_phc,
    )
    .is_ok();
    if !matches {
        return VerifyOutcome::Invalid;
    }
    if is_current_policy(&parsed) {
        VerifyOutcome::Valid
    } else {
        VerifyOutcome::RehashNeeded
    }
}

fn is_current_policy(parsed: &argon2::password_hash::phc::PasswordHash) -> bool {
    if parsed.algorithm != argon2::ARGON2ID_IDENT || parsed.version != Some(POLICY_VERSION_DECIMAL)
    {
        return false;
    }
    match Params::try_from(&parsed.params) {
        Ok(params) => {
            params.m_cost() >= POLICY_M_COST_KIB
                && params.t_cost() >= POLICY_T_COST
                && params.p_cost() >= POLICY_P_COST
        }
        Err(_) => false,
    }
}

/// Verifies with uniform timing when the account may not exist: hashes
/// against a process-lifetime dummy hash so "no such user" and "wrong
/// password" cost the same and return the same generic outcome.
pub fn verify_or_dummy(password: &str, stored: Option<&str>) -> bool {
    match stored {
        Some(phc) => verify_password(password, phc) != VerifyOutcome::Invalid,
        None => {
            static DUMMY: OnceLock<Option<String>> = OnceLock::new();
            let dummy =
                DUMMY.get_or_init(|| hash_password("dummy password for timing parity").ok());
            match dummy {
                Some(phc) => verify_password(password, phc) == VerifyOutcome::Valid,
                None => false,
            }
        }
    }
}

/// Length-only password policy check for registration and reset forms.
/// No composition rules: length bounds only, per the documented decision.
pub fn validate_password_length(password: &str) -> Result<(), PasswordError> {
    let len = password.chars().count();
    if (PASSWORD_MIN_LEN..=PASSWORD_MAX_LEN).contains(&len) {
        Ok(())
    } else {
        Err(PasswordError::LengthOutOfBounds)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use argon2::{Algorithm, Version};

    #[test]
    fn hash_round_trip_is_valid_under_policy() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(
            hash.starts_with("$argon2id$"),
            "PHC argon2id prefix: {hash}"
        );
        assert_eq!(
            verify_password("correct horse battery staple", &hash),
            VerifyOutcome::Valid
        );
    }

    #[test]
    fn wrong_password_is_invalid() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert_eq!(
            verify_password("wrong horse battery staple", &hash),
            VerifyOutcome::Invalid
        );
    }

    #[test]
    fn malformed_stored_hash_is_invalid_not_an_error() {
        assert_eq!(
            verify_password("whatever", "not-a-phc-string"),
            VerifyOutcome::Invalid
        );
        assert_eq!(verify_password("whatever", ""), VerifyOutcome::Invalid);
    }

    #[test]
    fn weaker_parameters_request_rehash() {
        // t_cost=1 is below the policy t_cost=2: same password, weaker hash.
        let weak = Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(19_456, 1, 1, None).expect("params"),
        );
        let salt = generate_salt();
        let hash = weak
            .hash_password_with_salt(b"correct horse battery staple", &salt)
            .expect("hash")
            .to_string();
        assert_eq!(
            verify_password("correct horse battery staple", &hash),
            VerifyOutcome::RehashNeeded
        );
    }

    #[test]
    fn non_argon2id_algorithm_requests_rehash() {
        let weak = Argon2::new(
            Algorithm::Argon2i,
            Version::V0x13,
            Params::new(19_456, 2, 1, None).expect("params"),
        );
        let salt = generate_salt();
        let hash = weak
            .hash_password_with_salt(b"correct horse battery staple", &salt)
            .expect("hash")
            .to_string();
        assert_eq!(
            verify_password("correct horse battery staple", &hash),
            VerifyOutcome::RehashNeeded
        );
    }

    #[test]
    fn password_length_bounds_are_enforced() {
        assert!(validate_password_length("twelvechars!").is_ok());
        assert_eq!(
            validate_password_length("short12"),
            Err(PasswordError::LengthOutOfBounds)
        );
        let long = "a".repeat(PASSWORD_MAX_LEN + 1);
        assert_eq!(
            validate_password_length(&long),
            Err(PasswordError::LengthOutOfBounds)
        );
    }

    #[test]
    fn dummy_verify_takes_the_uniform_invalid_path() {
        // No stored hash: must return false through the dummy hash work.
        assert!(!verify_or_dummy("whatever", None));
        // And the dummy path hashes, so both branches cost a real Argon2 op.
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(verify_or_dummy("correct horse battery staple", Some(&hash)));
    }
}
