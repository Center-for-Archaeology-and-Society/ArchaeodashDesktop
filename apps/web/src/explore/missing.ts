/**
 * Missing-value profile computed from rows already in memory, so the Explore
 * tab needs no backend round trip. Same rule as the backend's
 * `profile_missing` port (crates/analysis/src/explore.rs): a measured value
 * is missing when it is NA, `pct_missing` is the fraction of rows, bands are
 * Good ≤ 5%, OK ≤ 40%, Bad ≤ 80%, else Remove, and rows are ordered by
 * descending missing count with ties kept in column order.
 */
import type { MissingProfileRow } from '@archaeodash/client';
import type { CombinedRows } from '../visualize/combine.ts';

export function missingBand(fraction: number): string {
  if (fraction <= 0.05) return 'Good';
  if (fraction <= 0.4) return 'OK';
  if (fraction <= 0.8) return 'Bad';
  return 'Remove';
}

/** One row per requested measured column; unknown columns are skipped. */
export function missingProfile(
  combined: Pick<CombinedRows, 'elementalColumns' | 'rows'>,
  columns: readonly string[],
): MissingProfileRow[] {
  const n = combined.rows.length;
  const indexed = columns
    .map((feature) => ({ feature, index: combined.elementalColumns.indexOf(feature) }))
    .filter((c) => c.index >= 0);
  const counts = new Array<number>(indexed.length).fill(0);
  for (const row of combined.rows) {
    for (let i = 0; i < indexed.length; i++) {
      const value = row.elemental[indexed[i]!.index];
      if (value === null || value === undefined || Number.isNaN(value)) counts[i]! += 1;
    }
  }
  const rows = indexed.map(({ feature }, i) => {
    const num = counts[i]!;
    const pct = n === 0 ? Number.NaN : num / n;
    return { feature, num_missing: num, pct_missing: pct, band: n === 0 ? 'Remove' : missingBand(pct) };
  });
  // Array.prototype.sort is stable, matching R's order() on ties.
  return rows.sort((a, b) => b.num_missing - a.num_missing);
}
