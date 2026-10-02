//! Password/session/token/email policy for the hosted control plane
//! (Section 11.1 of `docs/implementation/11-authentication-privacy-security.md`).
//!
//! This slice owns the pure auth primitives:
//!
//! - [`password`]: Argon2id hashing policy with verify-and-rehash detection
//!   and an enumeration-resistant dummy-verify path.
//! - [`token`]: 256-bit opaque session/account tokens, SHA-256 storage
//!   digests, constant-time comparison, and one-time-link expiry rules.
//! - [`identity`]: normalized username/email rules and the password
//!   length policy.
//! - [`throttle`]: privacy-minimized throttle keys (HMAC-digested
//!   identifiers, never raw emails/IPs) and the fixed-window rate-limit
//!   policy with a replica-persistable store contract.
//!
//! The PostgreSQL-backed session/verification/throttle stores that implement
//! the persistence side of these contracts arrive with the control-plane
//! crate (Section 6.5); libsodium hash verification for legacy migration is
//! Phase 8 work and plugs into [`password::VerifyOutcome::RehashNeeded`].

pub mod identity;
pub mod password;
pub mod throttle;
pub mod token;

/// Crate version reported by the smoke use case.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

/// Placeholder smoke function proving the workspace links end to end.
pub fn smoke() -> &'static str {
    CRATE_NAME
}

/// Uniform, enumeration-resistant message for every login, verification,
/// and reset failure presented to clients (Section 11.1: "Use uniform
/// messages and timing where enumeration matters"). Callers must return
/// this exact string; detailed reasons stay in server logs.
pub const GENERIC_AUTH_MESSAGE: &str =
    "If the address and details are correct, the message was sent or the sign-in failed.";
