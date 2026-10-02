//! Privacy-minimized throttle keys and the persistent rate-limit contract
//! (Section 11.1: "Rate-limit login by privacy-minimized IP/account key and
//! reset/verification by IP/account/email, persistently across replicas").
//!
//! Throttle keys are HMAC-SHA256 digests of `category || 0x00 || identifier`
//! under a server-side pepper, so a leaked control-plane table never reveals
//! raw emails, usernames, or IP addresses. The fixed-window policy is
//! expressed against a store trait so the control-plane crate can implement
//! it with a single atomic upsert across replicas; the in-memory store here
//! exists for tests and local development.

use getrandom::fill;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// Throttle categories, one tag each so key domains cannot collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleCategory {
    LoginByAccount,
    LoginByIp,
    VerifyByAccount,
    VerifyByEmail,
    ResetByAccount,
    ResetByEmail,
}

impl ThrottleCategory {
    fn tag(self) -> &'static str {
        match self {
            ThrottleCategory::LoginByAccount => "login:account",
            ThrottleCategory::LoginByIp => "login:ip",
            ThrottleCategory::VerifyByAccount => "verify:account",
            ThrottleCategory::VerifyByEmail => "verify:email",
            ThrottleCategory::ResetByAccount => "reset:account",
            ThrottleCategory::ResetByEmail => "reset:email",
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PepperError {
    #[error("pepper must be at least 32 bytes of hex or generated fresh")]
    Invalid,
}

/// Server-side pepper for throttle-key derivation. Generated from the OS RNG
/// or loaded from configuration/secret storage as hex; never persisted next
/// to the digests it protects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThrottlePepper([u8; 32]);

impl ThrottlePepper {
    pub fn generate() -> Result<Self, PepperError> {
        let mut bytes = [0u8; 32];
        fill(&mut bytes).map_err(|_| PepperError::Invalid)?;
        Ok(Self(bytes))
    }

    /// Loads a hex-encoded pepper (64 hex chars). Rejects short or invalid
    /// input rather than silently weakening the derivation.
    pub fn from_hex(hex_str: &str) -> Result<Self, PepperError> {
        let bytes = decode_hex(hex_str)?;
        if bytes.len() != 32 {
            return Err(PepperError::Invalid);
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self(key))
    }

    /// Derives the privacy-minimized throttle key for one attempt in one
    /// category. The raw identifier is never stored anywhere.
    pub fn key(&self, category: ThrottleCategory, identifier: &str) -> [u8; 32] {
        hmac_sha256(
            &self.0,
            format!("{}{}{}", category.tag(), 0u8 as char, identifier).as_bytes(),
        )
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>, PepperError> {
    if !s.len().is_multiple_of(2) {
        return Err(PepperError::Invalid);
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| PepperError::Invalid))
        .collect()
}

/// HMAC-SHA256 (RFC 2104) over SHA-256's 64-byte block size.
fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut padded = [0x36u8; BLOCK]; // ipad
    let mut opad = [0x5cu8; BLOCK]; // opad
    for i in 0..BLOCK {
        let k = if i < key.len() { key[i] } else { 0 };
        padded[i] ^= k;
        opad[i] ^= k;
    }
    let mut inner = Sha256::new();
    inner.update(padded);
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

/// One fixed window of attempts per key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottlePolicy {
    pub max_attempts: u32,
    pub window: Duration,
}

impl ThrottlePolicy {
    pub const fn new(max_attempts: u32, window_secs: u64) -> Self {
        Self {
            max_attempts,
            window: Duration::from_secs(window_secs),
        }
    }
}

/// Named defaults (Section 11.1). Tuning happens here, one place.
pub const LOGIN_PER_ACCOUNT: ThrottlePolicy = ThrottlePolicy::new(10, 15 * 60);
pub const LOGIN_PER_IP: ThrottlePolicy = ThrottlePolicy::new(50, 15 * 60);
pub const VERIFY_PER_ACCOUNT: ThrottlePolicy = ThrottlePolicy::new(5, 24 * 60 * 60);
pub const VERIFY_PER_EMAIL: ThrottlePolicy = ThrottlePolicy::new(5, 24 * 60 * 60);
pub const RESET_PER_ACCOUNT: ThrottlePolicy = ThrottlePolicy::new(5, 60 * 60);
pub const RESET_PER_EMAIL: ThrottlePolicy = ThrottlePolicy::new(5, 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleDecision {
    Allowed,
    Limited { retry_after: Duration },
}

/// Persistence contract for throttle counters. Implementations must be
/// atomic across replicas (single conditional upsert in PostgreSQL), which
/// is what makes the limit persistent rather than per-process. Async so the
/// database-backed store can participate without blocking threads.
pub trait ThrottleStore: Send + Sync {
    /// Records one attempt against `key` and decides whether it is allowed
    /// within `policy`'s fixed window anchored at `now`.
    fn record_and_check(
        &self,
        key: &[u8; 32],
        policy: ThrottlePolicy,
        now: SystemTime,
    ) -> impl Future<Output = ThrottleDecision> + Send;

    /// Clears the counter for `key` (successful login/verify/reset).
    fn clear(&self, key: &[u8; 32]) -> impl Future<Output = ()> + Send;
}

/// In-memory fixed-window store for tests and local development. The
/// control-plane crate replaces this with the PostgreSQL implementation.
#[derive(Default)]
pub struct InMemoryThrottleStore {
    windows: std::sync::Mutex<std::collections::HashMap<[u8; 32], WindowState>>,
}

#[derive(Debug, Clone, Copy)]
struct WindowState {
    bucket: u64,
    count: u32,
}

impl InMemoryThrottleStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ThrottleStore for InMemoryThrottleStore {
    async fn record_and_check(
        &self,
        key: &[u8; 32],
        policy: ThrottlePolicy,
        now: SystemTime,
    ) -> ThrottleDecision {
        let bucket = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|t| t.as_secs() / policy.window.max(Duration::from_secs(1)).as_secs())
            .unwrap_or(0);
        // Poisoning cannot leave the counters unusable: proceed with the
        // guarded data even if another thread panicked mid-update.
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = windows
            .entry(*key)
            .or_insert(WindowState { bucket, count: 0 });
        if state.bucket != bucket {
            *state = WindowState { bucket, count: 0 };
        }
        state.count += 1;
        if state.count > policy.max_attempts {
            let window = policy.window.max(Duration::from_secs(1));
            let bucket_start = bucket * window.as_secs();
            let now_secs = now
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|t| t.as_secs())
                .unwrap_or(bucket_start);
            ThrottleDecision::Limited {
                retry_after: Duration::from_secs(
                    (bucket_start + window.as_secs())
                        .saturating_sub(now_secs)
                        .max(1),
                ),
            }
        } else {
            ThrottleDecision::Allowed
        }
    }

    async fn clear(&self, key: &[u8; 32]) {
        // Poisoning cannot leave the counters unusable: proceed with the
        // guarded data even if another thread panicked mid-update.
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key);
    }
}
#[cfg(test)]
mod tests {

    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    fn epoch(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn hmac_matches_rfc4231_case1() {
        // RFC 4231 test case 1: key = 20 bytes of 0x0b, message "Hi There".
        let mut key = [0x0bu8; 32];
        // Truncated-key variant: HMAC pads keys shorter than the block, and
        // a 32-byte key is used as-is. RFC 4231 case 1 uses a 20-byte key;
        // replicate its computation with the padded form.
        key[20..].fill(0);
        let mac = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            hex::encode(mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn keys_never_reveal_identifiers() {
        let pepper = ThrottlePepper::generate().expect("rng");
        let key = pepper.key(ThrottleCategory::LoginByAccount, "user@example.com");
        let text = String::from_utf8_lossy(&key);
        assert!(!text.contains("user@example.com"));
        // Same category+identifier is deterministic; different category differs.
        assert_eq!(
            key,
            pepper.key(ThrottleCategory::LoginByAccount, "user@example.com")
        );
        assert_ne!(
            key,
            pepper.key(ThrottleCategory::LoginByIp, "user@example.com")
        );
        assert_ne!(
            key,
            ThrottlePepper::generate()
                .expect("rng")
                .key(ThrottleCategory::LoginByAccount, "user@example.com")
        );
    }

    #[test]
    fn pepper_from_hex_round_trips_and_rejects_bad_input() {
        let hex_str = hex::encode([7u8; 32]);
        assert!(ThrottlePepper::from_hex(&hex_str).is_ok());
        assert_eq!(ThrottlePepper::from_hex("zz"), Err(PepperError::Invalid));
        assert_eq!(ThrottlePepper::from_hex("ab"), Err(PepperError::Invalid));
    }

    #[tokio::test]
    async fn window_limits_then_resets() {
        let store = InMemoryThrottleStore::new();
        let pepper = ThrottlePepper::generate().expect("rng");
        let key = pepper.key(ThrottleCategory::ResetByEmail, "a@b.co");
        let policy = ThrottlePolicy::new(3, 60);
        for _ in 0..3 {
            assert_eq!(
                store.record_and_check(&key, policy, epoch(1000)).await,
                ThrottleDecision::Allowed
            );
        }
        assert_eq!(
            store.record_and_check(&key, policy, epoch(1000)).await,
            ThrottleDecision::Limited {
                retry_after: Duration::from_secs(20)
            }
        );
        // Next window resets the counter.
        assert_eq!(
            store.record_and_check(&key, policy, epoch(1061)).await,
            ThrottleDecision::Allowed
        );
    }

    #[tokio::test]
    async fn clear_restores_allowance() {
        let store = InMemoryThrottleStore::new();
        let pepper = ThrottlePepper::generate().expect("rng");
        let key = pepper.key(ThrottleCategory::LoginByAccount, "a@b.co");
        let policy = ThrottlePolicy::new(1, 60);
        assert_eq!(
            store.record_and_check(&key, policy, epoch(0)).await,
            ThrottleDecision::Allowed
        );
        assert!(matches!(
            store.record_and_check(&key, policy, epoch(0)).await,
            ThrottleDecision::Limited { .. }
        ));
        store.clear(&key).await;
        assert_eq!(
            store.record_and_check(&key, policy, epoch(0)).await,
            ThrottleDecision::Allowed
        );
    }
}
