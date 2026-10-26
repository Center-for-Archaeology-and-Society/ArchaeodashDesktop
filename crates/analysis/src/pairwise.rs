//! Pairwise distances computed on the fly, in parallel row blocks, so large
//! inputs (up to ~100,000 rows) never materialise an n x n matrix.
//!
//! Each distance is accumulated over columns in the same order as the dense
//! helpers (`dense_euclidean`, `metric_matrix`), so results are bit-identical
//! to the matrix-based code paths.

use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use archaeodash_domain::DomainError;

use crate::cluster::DistanceMetric;

/// Columns of one tile of candidate rows `j`, sized to stay cache resident.
pub(crate) const TILE_ROWS: usize = 256;

/// Writes `d(i, j)` for every `j` in `j_range` into `out` (same length).
/// `cols` is column-major, as in `ColumnMatrix::cols`.
pub(crate) fn tile_distances(
    cols: &[Vec<f64>],
    metric: DistanceMetric,
    i: usize,
    j_range: Range<usize>,
    out: &mut [f64],
) {
    out.fill(0.0);
    match metric {
        DistanceMetric::Euclidean => {
            for col in cols {
                let xi = col[i];
                for (acc, &xj) in out.iter_mut().zip(&col[j_range.clone()]) {
                    let d = xi - xj;
                    *acc += d * d;
                }
            }
            for acc in out.iter_mut() {
                *acc = acc.sqrt();
            }
        }
        DistanceMetric::Manhattan => {
            for col in cols {
                let xi = col[i];
                for (acc, &xj) in out.iter_mut().zip(&col[j_range.clone()]) {
                    *acc += (xi - xj).abs();
                }
            }
        }
        DistanceMetric::Maximum => {
            for col in cols {
                let xi = col[i];
                for (acc, &xj) in out.iter_mut().zip(&col[j_range.clone()]) {
                    *acc = acc.max((xi - xj).abs());
                }
            }
        }
        DistanceMetric::Minkowski { p } => {
            for col in cols {
                let xi = col[i];
                for (acc, &xj) in out.iter_mut().zip(&col[j_range.clone()]) {
                    *acc += (xi - xj).abs().powf(p);
                }
            }
            for acc in out.iter_mut() {
                *acc = acc.powf(1.0 / p);
            }
        }
    }
}

/// Runs `work` over consecutive row blocks of `0..n` on all available cores
/// and returns the block results in row order. The first error (usually a
/// cancellation) stops the remaining blocks.
pub(crate) fn par_row_blocks<T, F>(n: usize, block: usize, work: F) -> Result<Vec<T>, DomainError>
where
    T: Send,
    F: Fn(Range<usize>) -> Result<T, DomainError> + Sync,
{
    let block = block.max(1);
    let blocks = n.div_ceil(block);
    if blocks == 0 {
        return Ok(Vec::new());
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(blocks);
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let work = &work;
    let next = &next;
    let failed = &failed;
    let mut slots: Vec<Option<T>> = (0..blocks).map(|_| None).collect();
    let mut first_error = None;
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    loop {
                        let b = next.fetch_add(1, Ordering::Relaxed);
                        if b >= blocks || failed.load(Ordering::Relaxed) {
                            return Ok(done);
                        }
                        let start = b * block;
                        match work(start..(start + block).min(n)) {
                            Ok(value) => done.push((b, value)),
                            Err(error) => {
                                failed.store(true, Ordering::Relaxed);
                                return Err(error);
                            }
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(Ok(done)) => {
                    for (b, value) in done {
                        slots[b] = Some(value);
                    }
                }
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
    if let Some(error) = first_error {
        return Err(error);
    }
    slots
        .into_iter()
        .map(|slot| {
            slot.ok_or_else(|| {
                DomainError::validation("analysis_worker", "a parallel analysis block did not run")
            })
        })
        .collect()
}
