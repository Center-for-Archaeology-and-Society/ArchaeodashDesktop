/**
 * The project's Analysis setup: which measured elements analyses and plots
 * use, and the transform (and later imputation) applied first. It is stored
 * as one reserved transformation definition, so the backend applies it on
 * demand from measured values and never writes derived values into group
 * files (Section 5 storage invariant).
 */
import type { TransformationDefinition, TransformMethod } from '@archaeodash/contracts';

/** Reserved definition name for the active setup (stored per project). */
export const ANALYSIS_SETUP_NAME = 'analysis_setup';

export const TRANSFORM_CHOICES: readonly { value: TransformMethod; label: string; hint: string }[] = [
  { value: 'none', label: 'None', hint: 'Measured values as imported.' },
  { value: 'log10', label: 'Log10', hint: 'Base-10 logarithm, the usual choice for INAA concentrations.' },
  { value: 'log', label: 'Natural log', hint: 'Base-e logarithm.' },
  { value: 'zScore', label: 'Z-score', hint: 'Percent of each row total, then standardized per element.' },
];

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
  parts.push(setup.transform_method === 'none' ? 'no transform' : transformLabel(setup.transform_method));
  return parts.join(' · ');
}

/** Union of measured columns across groups, in first-seen order. */
export function unionElements(lists: readonly (readonly string[])[]): string[] {
  const out: string[] = [];
  for (const list of lists) for (const c of list) if (!out.includes(c)) out.push(c);
  return out;
}
