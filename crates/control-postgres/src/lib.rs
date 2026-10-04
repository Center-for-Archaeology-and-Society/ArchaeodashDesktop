//! Hosted control-plane repository (SQLx per Section 6.5 of
//! `docs/implementation/06-storage-architecture.md`). Stores no
//! analytical-unit rows.
//!
//! Phase 7 slice: the auth control-plane schema (users, sessions,
//! account_tokens, auth_throttles), the PostgreSQL [`ThrottleStore`],
//! session rotation, and single-use account tokens. Catalog/projects/files
//! tables arrive with the hosted data slices of Phase 7.

mod store;

pub use store::{
    AccountTokenKind, ControlError, ControlStore, FileRow, ProjectRow, QuotaOutcome, SessionRow,
    SweptFile, UserRow,
};

/// Crate version reported by the smoke use case.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

/// Placeholder smoke function proving the workspace links end to end.
pub fn smoke() -> &'static str {
    CRATE_NAME
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_reports_crate_name() {
        assert_eq!(super::smoke(), super::CRATE_NAME);
    }
}
