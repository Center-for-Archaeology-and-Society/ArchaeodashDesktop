//! Normalized username/email rules and the password length policy
//! (Section 11.1: "Enforce normalized username character/length rules and
//! unique normalized email").
//!
//! Normalization produces the identity used for uniqueness checks and
//! throttle keys. The original email spelling is retained by the caller for
//! delivery; only the normalized form is indexed unique.

use thiserror::Error;

pub const USERNAME_MIN_LEN: usize = 3;
pub const USERNAME_MAX_LEN: usize = 40;
pub const EMAIL_MAX_LEN: usize = 254;
pub const EMAIL_LOCAL_MAX_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("username must be {USERNAME_MIN_LEN}-{USERNAME_MAX_LEN} characters of a-z, 0-9, dot, underscore, or hyphen, starting and ending alphanumeric")]
    UsernameInvalid,
    #[error("email address is not structurally valid")]
    EmailInvalid,
}

/// Normalizes a username: trims surrounding whitespace and lowercases.
/// Allowed characters are `a-z0-9._-`; the first and last characters must be
/// alphanumeric, and consecutive dots are rejected. Callers enforce
/// uniqueness of the normalized form in the control plane.
pub fn normalize_username(raw: &str) -> Result<String, IdentityError> {
    let name = raw.trim().to_lowercase();
    let len = name.len();
    if !(USERNAME_MIN_LEN..=USERNAME_MAX_LEN).contains(&len) {
        return Err(IdentityError::UsernameInvalid);
    }
    let ok_char =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-');
    if !name.chars().all(ok_char) {
        return Err(IdentityError::UsernameInvalid);
    }
    // Length was validated as non-empty, so first/last characters always exist.
    let starts_alnum = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    let ends_alnum = name
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    if !starts_alnum || !ends_alnum {
        return Err(IdentityError::UsernameInvalid);
    }
    if name.contains("..") {
        return Err(IdentityError::UsernameInvalid);
    }
    Ok(name)
}

/// Normalizes an email: trims, lowercases, and structurally validates
/// (exactly one `@`, non-empty local part up to 64 characters, domain with
/// at least one dot and valid labels, total up to 254 characters). This is
/// a uniqueness/pragmatism rule, not full RFC 5321 parsing.
pub fn normalize_email(raw: &str) -> Result<String, IdentityError> {
    let email = raw.trim().to_lowercase();
    if email.is_empty() || email.len() > EMAIL_MAX_LEN {
        return Err(IdentityError::EmailInvalid);
    }
    let Some((local, domain)) = email.split_once('@') else {
        return Err(IdentityError::EmailInvalid);
    };
    if domain.contains('@') || local.is_empty() || local.len() > EMAIL_LOCAL_MAX_LEN {
        return Err(IdentityError::EmailInvalid);
    }
    validate_domain(domain)?;
    Ok(email)
}

fn validate_domain(domain: &str) -> Result<(), IdentityError> {
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return Err(IdentityError::EmailInvalid);
    }
    for label in labels {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(IdentityError::EmailInvalid);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    #[test]
    fn usernames_normalize_and_validate() {
        assert_eq!(
            normalize_username("  Alice_Smith ").as_deref(),
            Ok("alice_smith")
        );
        assert_eq!(normalize_username("A.B-9").as_deref(), Ok("a.b-9"));
        assert_eq!(
            normalize_username("ab"),
            Err(IdentityError::UsernameInvalid)
        );
        assert_eq!(
            normalize_username(&"a".repeat(USERNAME_MAX_LEN + 1)),
            Err(IdentityError::UsernameInvalid)
        );
        assert_eq!(
            normalize_username(".leading"),
            Err(IdentityError::UsernameInvalid)
        );
        assert_eq!(
            normalize_username("trailing."),
            Err(IdentityError::UsernameInvalid)
        );
        assert_eq!(
            normalize_username("double..dot"),
            Err(IdentityError::UsernameInvalid)
        );
        assert_eq!(
            normalize_username("has space"),
            Err(IdentityError::UsernameInvalid)
        );
        assert_eq!(
            normalize_username("héllo"),
            Err(IdentityError::UsernameInvalid)
        );
        assert_eq!(normalize_username(""), Err(IdentityError::UsernameInvalid));
    }

    #[test]
    fn emails_normalize_and_validate() {
        assert_eq!(
            normalize_email("  User@Example.COM ").as_deref(),
            Ok("user@example.com")
        );
        assert_eq!(normalize_email("a@b.co").as_deref(), Ok("a@b.co"));
        assert_eq!(
            normalize_email("no-at-sign"),
            Err(IdentityError::EmailInvalid)
        );
        assert_eq!(
            normalize_email("two@@example.com"),
            Err(IdentityError::EmailInvalid)
        );
        assert_eq!(
            normalize_email("@example.com"),
            Err(IdentityError::EmailInvalid)
        );
        assert_eq!(normalize_email("user@"), Err(IdentityError::EmailInvalid));
        assert_eq!(
            normalize_email("user@nodot"),
            Err(IdentityError::EmailInvalid)
        );
        assert_eq!(
            normalize_email("user@-bad-.com"),
            Err(IdentityError::EmailInvalid)
        );
        assert_eq!(
            normalize_email(&format!("{}@x.co", "a".repeat(EMAIL_LOCAL_MAX_LEN + 1))),
            Err(IdentityError::EmailInvalid)
        );
        assert_eq!(
            normalize_email(&format!("u@{}.com", "b".repeat(EMAIL_MAX_LEN))),
            Err(IdentityError::EmailInvalid)
        );
    }

    #[test]
    fn boundary_lengths_are_accepted() {
        assert!(normalize_username(&"a".repeat(USERNAME_MAX_LEN)).is_ok());
        assert!(normalize_email(&format!("{}@x.co", "a".repeat(EMAIL_LOCAL_MAX_LEN))).is_ok());
        let domain = format!("{}.co", "b".repeat(63));
        assert!(normalize_email(&format!("u@{domain}")).is_ok());
    }
}
