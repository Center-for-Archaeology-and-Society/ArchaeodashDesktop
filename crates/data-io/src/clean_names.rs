//! Compatibility port of `janitor::clean_names(case = "none")` as used by the
//! legacy `dataLoader` (IMPLEMENTATION.md Section 7.2 item 6).
//!
//! Pipeline reproduced from the pinned oracle environment (janitor 2.2.0):
//!
//! 1. `str_replace_all` with the default `replace` map: `'` -> "", `"` -> "",
//!    `%` -> `_percent_`, `#` -> `_number_`.
//! 2. Latin-ASCII transliteration (`ascii = TRUE`). ASCII input is unchanged;
//!    non-ASCII transliteration is fixture-gated follow-up work (Section 15.1
//!    edge cases) and currently passes bytes through.
//! 3. Strip leading whitespace/punctuation/symbol/separator/other characters.
//! 4. Collapse remaining runs of those characters to a single `.`.
//! 5. R `make.names`: empty -> `X`, leading digit (or dot-digit) -> `X` prefix,
//!    reserved words get a trailing `.`.
//! 6. `snakecase::to_any_case(case = "none", sep_in = "\\.")`: split on `.`
//!    runs, no case conversion, join with `_`.
//! 7. Deduplicate with `_2`, `_3`, ... suffixes, recomputing collisions each
//!    pass exactly like `make_clean_names`.

const SPECIAL_REPLACEMENTS: [(&str, &str); 4] =
    [("'", ""), ("\"", ""), ("%", "_percent_"), ("#", "_number_")];

const R_RESERVED_WORDS: [&str; 20] = [
    "if",
    "else",
    "repeat",
    "while",
    "function",
    "for",
    "in",
    "next",
    "break",
    "TRUE",
    "FALSE",
    "NULL",
    "Inf",
    "NaN",
    "NA",
    "NA_integer_",
    "NA_real_",
    "NA_character_",
    "NA_complex_",
    "...",
];

fn is_strippable_start(c: char) -> bool {
    // \h \s \p{Punctuation} \p{Symbol} \p{Separator} \p{Other} at the start.
    // ASCII approximation: whitespace and everything that is not
    // alphanumeric/underscore... underscore IS punctuation (Pc) and is
    // stripped at the start and collapsed inside runs, matching janitor.
    c.is_whitespace() || (!c.is_alphanumeric() && c != '.')
}

fn is_collapsible(c: char) -> bool {
    // Runs of punctuation/symbol/separator/other become "."; letters, digits
    // and marks survive. ASCII-accurate for the documented fixture scope.
    c.is_whitespace() || (!c.is_alphanumeric() && c != '.')
}

fn make_r_name(name: &str) -> String {
    if name.is_empty() {
        return "X".to_string();
    }
    let Some(first) = name.chars().next() else {
        return "X".to_string();
    };
    let starts_bad = first.is_ascii_digit()
        || (first == '.' && name.chars().nth(1).is_some_and(|c| c.is_ascii_digit()));
    if starts_bad {
        return format!("X{name}");
    }
    if R_RESERVED_WORDS.contains(&name) {
        return format!("{name}.");
    }
    name.to_string()
}

/// Applies the `case = "none"` cleaning pipeline to one name.
pub fn clean_name_case_none(raw: &str) -> String {
    // 1. special-character replacements, in map order.
    let mut s = raw.to_string();
    for (from, to) in SPECIAL_REPLACEMENTS {
        s = s.replace(from, to);
    }
    // 2. transliteration: identity for ASCII (see module docs).
    // 3. strip leading punctuation/space/symbol run.
    let stripped = s.trim_start_matches(is_strippable_start);
    // 4. collapse interior runs to ".".
    let mut collapsed = String::with_capacity(stripped.len());
    let mut in_run = false;
    for c in stripped.chars() {
        if is_collapsible(c) {
            if !in_run {
                collapsed.push('.');
                in_run = true;
            }
        } else {
            collapsed.push(c);
            in_run = false;
        }
    }
    // 5. make.names validity pass.
    let made = make_r_name(&collapsed);
    // 6. split on "." runs, join with "_" (case untouched).
    let joined = made
        .split('.')
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    // Trailing dot from the reserved-word rule must survive the split:
    // make.names("if") -> "if." -> segments ["if"] -> "if". Reproduce R by
    // keeping the reserved-word marker: R's to_any_case receives "if." and
    // parses it to "if" too, so no extra handling is needed.
    joined
}

/// Deduplicates exactly like `make_clean_names(allow_dupes = FALSE)`.
///
/// R computes all duplicate counts against a snapshot of the current vector,
/// then applies every rename in the same pass, repeating until stable:
/// `["as","as","as"]` -> `["as","as_2","as_3"]` (verified against the oracle).
pub fn dedupe_names(names: Vec<String>) -> Vec<String> {
    let mut out = names;
    loop {
        let snapshot = out.clone();
        let mut changed = false;
        for (i, name) in snapshot.iter().enumerate() {
            let count = snapshot[..=i].iter().filter(|n| *n == name).count();
            if count > 1 {
                out[i] = format!("{name}_{count}");
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    out
}

/// `janitor::clean_names(frame, case = "none")` applied to a header vector.
pub fn clean_names_case_none(names: &[String]) -> Vec<String> {
    dedupe_names(names.iter().map(|n| clean_name_case_none(n)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_headers_match_golden() {
        let raw = [
            "anid",
            "site_name",
            "site_numbe",
            "Sub-region",
            "cer_type",
            "Cer_grp",
            "Ware",
            "CORE",
            "as",
            "la",
            "lu",
            "nd",
            "sm",
            "u",
            "yb",
            "ce",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
        let cleaned = clean_names_case_none(&raw);
        let expected = [
            "anid",
            "site_name",
            "site_numbe",
            "Sub_region",
            "cer_type",
            "Cer_grp",
            "Ware",
            "CORE",
            "as",
            "la",
            "lu",
            "nd",
            "sm",
            "u",
            "yb",
            "ce",
        ];
        assert_eq!(cleaned, expected);
    }

    #[test]
    fn edge_cases_follow_janitor_pipeline() {
        // Expected values verified against janitor 2.2.0 in the oracle env.
        assert_eq!(
            dedupe_names(vec![
                clean_name_case_none(""),
                clean_name_case_none("2020 data"),
                clean_name_case_none("% off"),
                clean_name_case_none("#num"),
                clean_name_case_none("  lead"),
                clean_name_case_none("if"),
                clean_name_case_none("a  b"),
                clean_name_case_none("a...b"),
                clean_name_case_none("ANID"),
            ]),
            vec![
                "X",
                "X2020_data",
                "percent_off",
                "number_num",
                "lead",
                "if",
                "a_b",
                "a_b_2", // collides with "a  b" -> "a_b" in the same vector
                "ANID",  // case="none" preserves case
            ]
        );
        // Standalone collision-free dedupe verified against the oracle.
        assert_eq!(
            dedupe_names(vec!["as".into(), "as".into(), "as".into()]),
            vec!["as", "as_2", "as_3"]
        );
        assert_eq!(
            dedupe_names(vec!["as".into(), "as_2".into(), "as".into()]),
            vec!["as", "as_2", "as_2_2"]
        );
    }
}
