//! Measured-data CSV export with data.table `fwrite`-compatible quoting
//! (golden #14, `saveexportTab.R` via `rio::export`).
//!
//! The legacy export path serializes an all-character frame: fields are
//! quoted only when necessary (separator, quote, or line break) and empty
//! strings are written as `""` so they stay distinguishable from `NA`, which
//! is written as an empty field. Numbers keep their source text because the
//! import pipeline is fully textual (`TextFrame`), so a round trip through
//! this writer is byte-exact against the oracle export.

use std::path::Path;

use crate::{ImportError, TextFrame};

/// Serializes one CSV field with `fwrite` `quote="necessary"` semantics.
///
/// `None` and `Some("")` both denote the empty string in an all-character
/// frame and are written as `""`; `NA` text is written literally because the
/// import pipeline never synthesizes it.
pub fn csv_field(cell: Option<&str>) -> String {
    let value = cell.unwrap_or("");
    let needs_quotes = value.is_empty()
        || value.contains(',')
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r');
    if needs_quotes {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Writes a `TextFrame` as CSV exactly like the legacy `rio::export` path:
/// UTF-8, `,` separator, `\n` line endings, trailing newline, `fwrite`
/// necessary-quoting. Byte-exactness against the golden export is asserted by
/// parity test `golden_14_export`.
pub fn write_text_csv(path: &Path, frame: &TextFrame) -> Result<(), ImportError> {
    let mut out = String::with_capacity(
        (frame.columns.iter().map(String::len).sum::<usize>() + frame.columns.len() * 4)
            * (frame.n_rows() + 1),
    );
    out.push_str(
        &frame
            .columns
            .iter()
            .map(|c| csv_field(Some(c)))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    for row in &frame.rows {
        let cells: Vec<String> = row.iter().map(|c| csv_field(c.as_deref())).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    write_atomic(path, out.as_bytes())
}

/// Write-to-temp, flush, atomic rename (Section 6.3 project metadata rule;
/// reused by every file writer so readers never observe partial output).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ImportError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ImportError::Io(e.to_string()))?;
        let tmp = parent.join(format!(".tmp-{}", uuid::Uuid::now_v7().simple()));
        std::fs::write(&tmp, bytes).map_err(|e| ImportError::Io(e.to_string()))?;
        // fsync before rename so the renamed file is durable on crash.
        if let Ok(f) = std::fs::File::open(&tmp) {
            let _ = f.sync_all();
        }
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            ImportError::Io(e.to_string())
        })?;
        sync_dir(parent);
    } else {
        std::fs::write(path, bytes).map_err(|e| ImportError::Io(e.to_string()))?;
    }
    Ok(())
}

/// Best-effort parent-directory fsync so renames survive power loss on
/// filesystems that support it (no-op where unsupported, e.g. Windows).
pub fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_only_when_necessary() {
        assert_eq!(csv_field(Some("plain")), "plain");
        assert_eq!(csv_field(Some("")), "\"\"");
        assert_eq!(csv_field(None), "\"\"");
        assert_eq!(csv_field(Some("a,b")), "\"a,b\"");
        assert_eq!(csv_field(Some("he\"s")), "\"he\"\"s\"");
        assert_eq!(csv_field(Some("a\nb")), "\"a\nb\"");
    }

    #[test]
    fn round_trip_preserves_cells() {
        let guard = tempfile::tempdir().expect("tempdir");
        let dir = guard.path();
        let source = dir.join("in.csv");
        std::fs::write(&source, "anid,note,as\nA1,,1.5\nA2,\"q\"\"x\",2\n").expect("write");
        let frame = crate::loader::data_loader(&source).expect("load");
        let out = dir.join("out.csv");
        write_text_csv(&out, &frame).expect("export");
        let reread = crate::loader::read_text_table(&out).expect("reread");
        assert_eq!(reread.columns, frame.columns);
        assert_eq!(reread.rows, frame.rows);
        let text = std::fs::read_to_string(&out).expect("text");
        assert!(text.ends_with('\n'));
        assert!(!text.contains("\r\n"));
    }
}
