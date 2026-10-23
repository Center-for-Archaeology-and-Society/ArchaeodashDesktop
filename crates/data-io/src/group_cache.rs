//! In-process cache of validated and decoded group files.
//!
//! Every analysis, Explore view, row listing, and validation used to re-open
//! and decode the same Parquet files (validation alone decodes once more), so
//! most request time went to I/O. This cache keeps the last decoded copy of
//! each file and serves it until the file changes.
//!
//! Freshness: the key is the canonical path plus the file's size,
//! modification time, and a SHA-256 of its Parquet footer. The footer holds
//! the group profile (revision id, measured checksum) and every column
//! chunk's offsets and statistics, so any transaction or external rewrite
//! changes the key even when the size and a coarse timestamp do not.
//!
//! Bounded to [`MAX_ENTRIES`] files and [`MAX_ROWS`] cached rows, evicting
//! the least recently used. `ARCHAEODASH_GROUP_CACHE=off` disables it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::group_profile::{GroupFileData, GroupProfile};

/// Most files kept.
pub const MAX_ENTRIES: usize = 128;
/// Most decoded rows kept across all files.
pub const MAX_ROWS: usize = 1_000_000;
/// Largest footer hashed; bigger footers are simply not cached.
const MAX_FOOTER: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    path: PathBuf,
    len: u64,
    modified_ns: u128,
    footer: [u8; 32],
}

struct Entry {
    key: Key,
    profile: GroupProfile,
    data: Option<GroupFileData>,
    last_used: u64,
}

#[derive(Default)]
struct Cache {
    entries: Vec<Entry>,
    clock: u64,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        !std::env::var("ARCHAEODASH_GROUP_CACHE").is_ok_and(|v| v.eq_ignore_ascii_case("off"))
    })
}

/// The cache key for `path`, or `None` (no caching) when the file cannot be
/// inspected or does not look like Parquet.
pub(crate) fn key_for(path: &Path) -> Option<Key> {
    if !enabled() {
        return None;
    }
    let path = path.canonicalize().ok()?;
    let mut file = File::open(&path).ok()?;
    let meta = file.metadata().ok()?;
    let len = meta.len();
    let modified_ns = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    // Parquet tail: footer, 4-byte little-endian footer length, "PAR1".
    if len < 12 {
        return None;
    }
    let mut tail = [0u8; 8];
    file.seek(SeekFrom::End(-8)).ok()?;
    file.read_exact(&mut tail).ok()?;
    if &tail[4..] != b"PAR1" {
        return None;
    }
    let footer_len = u64::from(u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]));
    if footer_len + 8 > len || footer_len > MAX_FOOTER {
        return None;
    }
    let mut footer = vec![0u8; usize::try_from(footer_len).ok()?];
    file.seek(SeekFrom::End(-8 - i64::try_from(footer_len).ok()?))
        .ok()?;
    file.read_exact(&mut footer).ok()?;
    let footer: [u8; 32] = Sha256::digest(&footer).into();
    Some(Key {
        path,
        len,
        modified_ns,
        footer,
    })
}

fn with_cache<T>(f: impl FnOnce(&mut Cache) -> T) -> T {
    // A poisoned lock only means another thread panicked mid-update; the
    // cache is advisory, so start over rather than propagate the panic.
    let mut guard = match cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = Cache::default();
            guard
        }
    };
    f(&mut guard)
}

fn touch(cache: &mut Cache, index: usize) {
    cache.clock += 1;
    cache.entries[index].last_used = cache.clock;
}

/// Cached profile for `key` (from either a validation or a full read).
pub(crate) fn profile(key: &Key) -> Option<GroupProfile> {
    with_cache(|c| {
        let index = c.entries.iter().position(|e| &e.key == key)?;
        touch(c, index);
        Some(c.entries[index].profile.clone())
    })
}

/// Cached decoded data for `key`.
pub(crate) fn data(key: &Key) -> Option<GroupFileData> {
    with_cache(|c| {
        let index = c
            .entries
            .iter()
            .position(|e| &e.key == key && e.data.is_some())?;
        touch(c, index);
        c.entries[index].data.clone()
    })
}

/// Records a successful validation.
pub(crate) fn put_profile(key: Key, profile: &GroupProfile) {
    put(key, profile.clone(), None);
}

/// Records a successful full read.
pub(crate) fn put_data(key: Key, data: &GroupFileData) {
    put(key, data.profile.clone(), Some(data.clone()));
}

fn put(key: Key, profile: GroupProfile, data: Option<GroupFileData>) {
    with_cache(|c| {
        // Older versions of the same file can never match again.
        c.entries.retain(|e| e.key.path != key.path || e.key == key);
        c.clock += 1;
        let clock = c.clock;
        match c.entries.iter_mut().find(|e| e.key == key) {
            Some(existing) => {
                existing.profile = profile;
                if data.is_some() {
                    existing.data = data;
                }
                existing.last_used = clock;
            }
            None => c.entries.push(Entry {
                key,
                profile,
                data,
                last_used: clock,
            }),
        }
        let rows = |c: &Cache| {
            c.entries
                .iter()
                .filter_map(|e| e.data.as_ref())
                .map(|d| d.rows.len())
                .sum::<usize>()
        };
        while c.entries.len() > MAX_ENTRIES || (rows(c) > MAX_ROWS && c.entries.len() > 1) {
            let oldest = c
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(i, _)| i)
                .unwrap_or(0);
            c.entries.remove(oldest);
        }
    });
}

/// Drops every cached file (tests and diagnostics).
pub fn clear() {
    with_cache(|c| c.entries.clear());
}
