import type { GroupRowsResponse, TransformationDefinition, AnalysisInputOptions } from '@archaeodash/client';

/** Only named, recomputed columns can be selected; persisted measured data stays untouched. */
export function analysisColumns(data: GroupRowsResponse, definition: TransformationDefinition | null): string[] {
  if (!definition) return data.elemental_columns;
  const ratios = definition.ratios.map(r => r.output_name ?? `${r.numerator}_${r.denominator}`.replace(/[^\p{L}\p{N}._-]/gu, '_'));
  return [...new Set(definition.ratio_mode === 'only' && ratios.length ? ratios : [...definition.elemental_columns, ...definition.ratios.flatMap(r => [r.numerator, r.denominator]), ...ratios])];
}

export function projectionLabels(data: GroupRowsResponse, column: string): string[] {
  const index = data.descriptive_columns.indexOf(column);
  return index < 0 ? [] : [...new Set(data.rows.map(row => row.descriptive[index]).filter((v): v is string => !!v))].sort();
}

export function sourceOptions(source: NonNullable<AnalysisInputOptions['source']>, count: number, group: string, seed: number): AnalysisInputOptions {
  return {
    source,
    pc_count: source === 'pca' ? count : null,
    source_group_column: source === 'lda' ? group : null,
    umap_seed: source === 'umap' ? seed : null,
  };
}
