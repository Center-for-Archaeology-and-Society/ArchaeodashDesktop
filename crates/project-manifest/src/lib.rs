//! Project metadata, journals, schema versions; index only, never an eligibility gate (Section 5.2).
//!
//! Stub created during Phase 1 (repository skeleton). Implemented in the
//! phase that owns this crate per `docs/implementation/04-repository-structure.md`.

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
