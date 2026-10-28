import type { GroupRowsResponse, TransformationDefinition, AnalysisInputOptions } from '@archaeodash/client';

/** Only named, recomputed columns can be selected; persisted measured data stays untouched. */
export function analysisColumns(data: GroupRowsResponse, definition: TransformationDefinition | null): string[] {
  if (!definition) return data.elemental_columns;
  const ratios = definition.ratios.map(r => r.output_name ?? `${r.numerator}_${r.denominator}`.replace(/[^\p{L}\p{N}._-]/gu, '_'));
  // Matches the backend's final columns: selected elements plus ratio outputs ('append'), or just the ratios ('only').
  return [...new Set(definition.ratio_mode === 'only' && ratios.length ? ratios : [...definition.elemental_columns, ...ratios])];
}

export function projectionLabels(data: GroupRowsResponse, column: string): string[] {
  const index = data.descriptive_columns.indexOf(column);
  return index < 0 ? [] : [...new Set(data.rows.map(row => row.descriptive[index]).filter((v): v is string => !!v))].sort();
}

/** `count` is the PCA component count or the UMAP dimension count. */
export function sourceOptions(source: NonNullable<AnalysisInputOptions['source']>, count: number, group: string, seed: number): AnalysisInputOptions {
  return {
    source,
    pc_count: source === 'pca' || source === 'umap' ? count : null,
    source_group_column: source === 'lda' ? group : null,
    umap_seed: source === 'umap' ? seed : null,
  };
}

/**
 * ID columns whose values identify every row (GroupMembership.R `sampleIDUI`,
 * EuclideanDistance.R `EDsampleIDUI`: unique-value count equals the row
 * count, a missing value counting once). Falls back to the visible ID.
 */
export function uniqueIdColumns(data: GroupRowsResponse): string[] {
  const n = data.rows.length;
  const columns = [...new Set([data.visible_id_column, ...data.descriptive_columns])];
  const unique = columns.filter(column => {
    const index = data.descriptive_columns.indexOf(column);
    const values = column === data.visible_id_column
      ? data.rows.map(row => row.visible_id ?? null)
      : data.rows.map(row => row.descriptive[index] ?? null);
    return new Set(values).size === n;
  });
  return unique.length ? unique : [data.visible_id_column];
}

/** Default ID column: the visible ID when unique, else an `anid` column, else the first choice. */
export function defaultIdColumn(data: GroupRowsResponse, choices: readonly string[]): string {
  if (choices.includes(data.visible_id_column)) return data.visible_id_column;
  return choices.find(c => c.toLowerCase() === 'anid') ?? choices[0] ?? data.visible_id_column;
}

export interface GroupEligibility {
  readonly group: string;
  readonly count: number;
  /** `getEligible`: more than `max(features, groups) + 1` rows. */
  readonly eligible: boolean;
}

/**
 * Row count and membership eligibility of every non-blank group in
 * `column`, in byte order (Group_probs.R `getEligible`; blank cells count
 * as one more group level, as in the Rust core).
 */
export function groupEligibility(data: GroupRowsResponse, column: string, featureCount: number): GroupEligibility[] {
  const index = data.descriptive_columns.indexOf(column);
  if (index < 0) return [];
  const counts = new Map<string, number>();
  for (const row of data.rows) {
    const label = row.descriptive[index] ?? '';
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  const threshold = Math.max(featureCount, counts.size) + 1;
  return [...counts]
    .filter(([group]) => group !== '')
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([group, count]) => ({ group, count, eligible: count > threshold }));
}

/**
 * Number of analysis features a source produces (the `n_features` in
 * eligibility). `count` is the PCA component or UMAP dimension count.
 */
export function sourceFeatureCount(source: NonNullable<AnalysisInputOptions['source']>, columns: number, count: number, ldaLevels: number): number {
  switch (source) {
    case 'pca':
    case 'umap': return count;
    case 'lda': return Math.max(1, Math.min(columns, ldaLevels - 1));
    default: return columns;
  }
}

/** PC-count choices labelled like pcaHelpers.R: "First n PCs (var% / cum%)". */
export function pcCountOptions(count: number, explained?: readonly number[], cumulative?: readonly number[]): { value: number; label: string }[] {
  const pct = (share: number | undefined) => (typeof share === 'number' && Number.isFinite(share) ? `${(share * 100).toFixed(1)}%` : null);
  return Array.from({ length: Math.max(0, count) }, (_, i) => {
    const own = pct(explained?.[i]);
    const total = pct(cumulative?.[i]);
    return { value: i + 1, label: own && total ? `First ${i + 1} PCs (${own}/${total})` : `First ${i + 1} PCs` };
  });
}
