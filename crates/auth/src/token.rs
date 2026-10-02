//! Opaque session/account tokens and their storage digests (Section 11.1).
//!
//! Tokens are 256 random bits from the OS RNG, shown once to the client as
//! base64url without padding. Storage keeps only the SHA-256 digest; a
//! database leak therefore reveals nothing usable for authentication.
//! Presentation tokens are compared in constant time.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use getrandom::fill;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// Session and account tokens carry 256 random bits (Section 11.1).
pub const TOKEN_BYTES: usize = 32;

/// Verification and reset links expire after 24 hours and are single-use
/// (the store consumes the digest on first presentation).
pub const ONE_TIME_LINK_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Remembered-session lifetimes offered to the user, in days
/// (Section 17.1 item 10: 30/90-day rotating remember-me). The session
/// cookie itself lasts only the browser session unless remember-me is chosen.
pub const REMEMBER_ME_DAY_CHOICES: [u64; 2] = [30, 90];
pub const REMEMBER_ME_DEFAULT_DAYS: u64 = 30;

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("token is not valid base64url of {TOKEN_BYTES} bytes")]
    Malformed,
    #[error("secure random generation failed")]
    RngFailed,
}

/// A freshly generated opaque token. Only `presentation` reaches the client;
/// the digest form is what a store persists.
#[derive(Debug, Clone)]
pub struct OpaqueToken(String);

impl OpaqueToken {
    /// Generates a 256-bit token from the OS RNG.
    pub fn generate() -> Result<Self, TokenError> {
        Ok(Self(random_token_string()?))
    }

    /// The base64url (unpadded) presentation form, shown once to the client.
    pub fn presentation(&self) -> &str {
        &self.0
    }

    /// SHA-256 digest to persist instead of the presentation token.
    pub fn digest(&self) -> [u8; 32] {
        digest_bytes(self.0.as_bytes())
    }
}

/// Generates one unpadded base64url token string of `TOKEN_BYTES` random bytes.
pub fn random_token_string() -> Result<String, TokenError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    fill(&mut bytes).map_err(|_| TokenError::RngFailed)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Digests a presented token string exactly as generated forms are digested.
/// Malformed presentations still digest (comparison then fails in constant
/// time), so callers can treat every presentation uniformly.
pub fn digest_presentation(presentation: &str) -> [u8; 32] {
    digest_bytes(presentation.as_bytes())
}

fn digest_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Constant-time equality for two 32-byte digests (no early exit on the
/// first differing byte).
pub fn digests_equal(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Whether a one-time link token has expired at `now`.
pub fn is_expired(expires_at: SystemTime, now: SystemTime) -> bool {
    now >= expires_at
}

/// Expiry instant for a one-time link issued at `now`.
pub fn one_time_expiry(now: SystemTime) -> SystemTime {
    now + ONE_TIME_LINK_TTL
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    #[test]
    fn generated_tokens_are_43_char_base64url() {
        let t = OpaqueToken::generate().expect("rng");
        assert_eq!(t.presentation().len(), 43);
        assert!(t
            .presentation()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }

    #[test]
    fn tokens_are_unique_across_generations() {
        let a = OpaqueToken::generate().expect("rng");
        let b = OpaqueToken::generate().expect("rng");
        assert_ne!(a.presentation(), b.presentation());
        assert!(!digests_equal(&a.digest(), &b.digest()));
    }

    #[test]
    fn digest_is_the_sha256_of_the_presentation() {
        let t = OpaqueToken::generate().expect("rng");
        let redigested = digest_presentation(t.presentation());
        assert!(digests_equal(&t.digest(), &redigested));
    }

    #[test]
    fn digest_comparison_is_exact() {
        let a = [7u8; 32];
        let b = [7u8; 32];
        let c = [8u8; 32];
        assert!(digests_equal(&a, &b));
        assert!(!digests_equal(&a, &c));
        // Differing only in the last byte is still unequal.
        let mut d = a;
        d[31] ^= 1;
        assert!(!digests_equal(&a, &d));
    }

    #[test]
    fn one_time_expiry_window_is_24_hours() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(one_time_expiry(now), now + ONE_TIME_LINK_TTL);
        assert!(!is_expired(one_time_expiry(now), now));
        assert!(is_expired(one_time_expiry(now), now + ONE_TIME_LINK_TTL));
    }

    #[test]
    fn remember_me_choices_match_section_17_1() {
        assert_eq!(REMEMBER_ME_DAY_CHOICES, [30, 90]);
        assert_eq!(REMEMBER_ME_DEFAULT_DAYS, 30);
    }
}
