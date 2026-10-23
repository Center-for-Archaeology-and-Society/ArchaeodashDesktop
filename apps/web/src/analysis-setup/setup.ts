/**
 * The project's Analysis setup: which measured elements analyses and plots
 * use, and the imputation and transform applied first. It is stored
 * as one reserved transformation definition, so the backend applies it on
 * demand from measured values and never writes derived values into group
 * files (Section 5 storage invariant).
 */
import type { ImputationMethod, TransformationDefinition, TransformMethod } from '@archaeodash/contracts';

/** Reserved definition name for the active setup (stored per project). */
export const ANALYSIS_SETUP_NAME = 'analysis_setup';

export const TRANSFORM_CHOICES: readonly { value: TransformMethod; label: string; hint: string }[] = [
  { value: 'none', label: 'None', hint: 'Measured values as imported.' },
  { value: 'log10', label: 'Log10', hint: 'Base-10 logarithm, the usual choice for INAA concentrations.' },
  { value: 'log', label: 'Natural log', hint: 'Base-e logarithm.' },
  { value: 'zScore', label: 'Z-score', hint: 'Percent of each row total, then standardized per element.' },
];

/** Legacy mice methods (Section 8.3), in the legacy app's order. */
export const IMPUTATION_CHOICES: readonly { value: ImputationMethod; label: string; short: string; hint: string }[] = [
  { value: 'none', label: 'None', short: 'no imputation', hint: 'Missing values stay missing; PCA and LDA then need complete rows.' },
  { value: 'rf', label: 'Random forest', short: 'random-forest imputation', hint: 'mice with 10-tree random-forest donors.' },
  { value: 'pmm', label: 'Predictive mean matching', short: 'PMM imputation', hint: 'mice predictive mean matching (5 donors).' },
  { value: 'midastouch', label: 'Weighted predictive mean matching', short: 'weighted PMM imputation', hint: 'mice midastouch: donors weighted by closeness.' },
];

/** Seed every page uses for UMAP, so precomputed and on-page results match. */
export const UMAP_SEED = 20260914;

/** Default imputation seed; shown and editable so results can be replayed. */
export const DEFAULT_IMPUTATION_SEED = 20260914;

export function imputationLabel(method: ImputationMethod): string {
  return IMPUTATION_CHOICES.find((c) => c.value === method)?.short ?? method;
}

export function transformLabel(method: TransformMethod): string {
  return TRANSFORM_CHOICES.find((c) => c.value === method)?.label ?? method;
}

/** A setup that selects `elements` with no transform. */
export function newSetup(elements: readonly string[]): TransformationDefinition {
  return {
    name: ANALYSIS_SETUP_NAME,
    transform_method: 'none',
    imputation_method: 'none',
    imputation_seed: null,
    elemental_columns: [...elements],
    descriptive_columns: [],
    group_column: null,
    ratios: [],
    ratio_mode: 'append',
    precompute: [],
  };
}

/** Whether the setup changes values (rather than only choosing columns). */
export function setupTransforms(setup: TransformationDefinition | null): boolean {
  return !!setup && (setup.transform_method !== 'none' || setup.imputation_method !== 'none');
}

export interface AnalysisInput {
  /** Columns to analyze, in the order the data lists them. */
  readonly columns: string[];
  /** Definition to send with the request, when values are transformed. */
  readonly transformation?: TransformationDefinition;
  /** Selected elements missing from these groups. */
  readonly missing: string[];
}

/**
 * Columns and transformation for an analysis over groups that measure
 * `available`. Without a setup every available column is used, as before.
 */
export function analysisInput(
  setup: TransformationDefinition | null,
  available: readonly string[],
): AnalysisInput {
  if (!setup) return { columns: [...available], missing: [] };
  const chosen = new Set(setup.elemental_columns);
  const columns = available.filter((c) => chosen.has(c));
  const missing = setup.elemental_columns.filter((c) => !available.includes(c));
  if (!setupTransforms(setup)) return { columns, missing };
  return { columns, missing, transformation: { ...setup, elemental_columns: columns } };
}

/** One-line summary for the Data Manager, e.g. "12 of 33 elements · Log10". */
export function setupSummary(setup: TransformationDefinition | null, allElements: readonly string[]): string {
  if (!setup) return `All ${allElements.length} elements · no transform`;
  const used = setup.elemental_columns.filter((c) => allElements.includes(c)).length;
  const parts = [`${used} of ${allElements.length} elements`];
  if (setup.imputation_method !== 'none') parts.push(imputationLabel(setup.imputation_method));
  parts.push(setup.transform_method === 'none' ? 'no transform' : transformLabel(setup.transform_method));
  return parts.join(' · ');
}

/** Union of measured columns across groups, in first-seen order. */
export function unionElements(lists: readonly (readonly string[])[]): string[] {
  const out: string[] = [];
  for (const list of lists) for (const c of list) if (!out.includes(c)) out.push(c);
  return out;
}
