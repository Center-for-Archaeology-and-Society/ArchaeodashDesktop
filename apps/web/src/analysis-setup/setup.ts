/**
 * The project's Analysis setup: which measured elements analyses and plots
 * use, and the imputation and transform applied first. It is stored
 * as one reserved transformation definition, so the backend applies it on
 * demand from measured values and never writes derived values into group
 * files (Section 5 storage invariant).
 */
import type { ImputationMethod, RatioMode, RatioSpecDto, TransformationDefinition, TransformMethod } from '@archaeodash/contracts';

/** Reserved definition name for the active setup (stored per project). */
export const ANALYSIS_SETUP_NAME = 'analysis_setup';

/** Saved names keep at most this many characters on disk (legacy limit). */
const MAX_NAME_LEN = 32;

/**
 * Filename-safe form of a name, as the backend stores it
 * (`sanitize_group_name`): letters, digits, `.`, `_` and `-` are kept, any
 * other character becomes `_`.
 */
export function sanitizeName(name: string): string {
  const safe = Array.from(name, (c) => (/[\p{L}\p{N}._-]/u.test(c) ? c : '_')).join('');
  return safe || 'group';
}

/**
 * Storage key of a saved setup name; names with the same key overwrite each
 * other. Names are saved trimmed, so the key is of the trimmed name.
 */
export function setupStorageKey(name: string): string {
  return Array.from(sanitizeName(name.trim())).slice(0, MAX_NAME_LEN).join('');
}

/** Why a name cannot be used for a saved setup, or '' when it can. */
export function setupNameProblem(name: string): string {
  if (!name.trim()) return 'Enter a name.';
  if (setupStorageKey(name) === ANALYSIS_SETUP_NAME) return `“${name.trim()}” is reserved for the active setup; choose another name.`;
  return '';
}

/** Output column name of a ratio: its own name, else sanitized `numerator_denominator`. */
export function ratioOutputName(spec: RatioSpecDto): string {
  return spec.output_name || sanitizeName(`${spec.numerator}_${spec.denominator}`);
}

/**
 * Ratio specs for every numerator × denominator pair (legacy
 * `expand_grid`), skipping a column divided by itself. A single pair may
 * carry a custom name; batches use `numerator_denominator`.
 */
export function ratioCombinations(numerators: readonly string[], denominators: readonly string[], name = ''): RatioSpecDto[] {
  const pairs = numerators.flatMap((numerator) => denominators.filter((d) => d !== numerator).map((denominator) => ({ numerator, denominator })));
  const single = numerators.length === 1 && denominators.length === 1;
  return pairs.map((pair) => ({ ...pair, output_name: single && name.trim() ? sanitizeName(name.trim()) : ratioOutputName(pair) }));
}

/** Adds ratios, replacing any existing ratio with the same output name. */
export function mergeRatios(current: readonly RatioSpecDto[], added: readonly RatioSpecDto[]): RatioSpecDto[] {
  const names = new Set(added.map(ratioOutputName));
  return [...current.filter((r) => !names.has(ratioOutputName(r))), ...added];
}

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

/** UMAP dimensions plots and precompute use. */
export const UMAP_DEFAULT_DIMENSIONS = 2;

/** Most UMAP dimensions offered (the backend's `UMAP_MAX_COMPONENTS`). */
export const UMAP_MAX_DIMENSIONS = 10;

/**
 * Most UMAP dimensions for `features` input columns, as the backend allows:
 * no more than the columns (extra axes add nothing), at least 2, at most 10.
 */
export function umapMaxDimensions(features: number): number {
  return Math.min(UMAP_MAX_DIMENSIONS, Math.max(UMAP_DEFAULT_DIMENSIONS, features));
}

/** Default imputation seed; shown and editable so results can be replayed. */
export const DEFAULT_IMPUTATION_SEED = 20260914;

export function imputationLabel(method: ImputationMethod): string {
  return IMPUTATION_CHOICES.find((c) => c.value === method)?.short ?? method;
}

/** "PMM imputation (all groups)" / "PMM imputation (included groups)". */
export function imputationSummary(setup: Pick<TransformationDefinition, 'imputation_method' | 'imputation_scope'>): string {
  const scope = (setup.imputation_scope ?? 'all_groups') === 'all_groups' ? 'all groups' : 'included groups';
  return `${imputationLabel(setup.imputation_method)} (${scope})`;
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
    imputation_scope: 'all_groups',
    elemental_columns: [...elements],
    descriptive_columns: [],
    group_column: null,
    ratios: [],
    ratio_mode: 'append',
    precompute: [],
  };
}

/**
 * Whether a sample's prepared values are the same whichever groups are
 * analyzed: a per-sample transform (not z-score, which standardizes over
 * the analyzed rows) and no imputation, or imputation from all groups. Such
 * values can be fetched once for the whole folder and reused.
 */
export function selectionIndependent(setup: TransformationDefinition | null | undefined): boolean {
  if (!setup || setup.transform_method === 'zScore') return false;
  return setup.imputation_method === 'none' || (setup.imputation_scope ?? 'all_groups') === 'all_groups';
}

/** Whether the setup changes values (rather than only choosing columns). */
export function setupTransforms(setup: TransformationDefinition | null): boolean {
  return !!setup && (setup.transform_method !== 'none' || setup.imputation_method !== 'none' || setup.ratios.length > 0);
}

export interface AnalysisInput {
  /**
   * Columns to analyze: the selected elements in the order the data lists
   * them, then ratio outputs (`append`), or only the ratio outputs (`only`).
   * Ratio columns exist only in the transformation's output.
   */
  readonly columns: string[];
  /** The selected measured elements these groups have (no ratio outputs). */
  readonly elements: string[];
  /** Definition to send with the request, when values are transformed. */
  readonly transformation?: TransformationDefinition;
  /** Selected elements, and ratios whose sources are, missing from these groups. */
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
  if (!setup) return { columns: [...available], elements: [...available], missing: [] };
  const chosen = new Set(setup.elemental_columns);
  const elements = available.filter((c) => chosen.has(c));
  const missing = setup.elemental_columns.filter((c) => !available.includes(c));
  // Like the legacy app, ratios whose sources these groups lack are skipped.
  const ratios = setup.ratios.filter((r) => available.includes(r.numerator) && available.includes(r.denominator));
  for (const r of setup.ratios) if (!ratios.includes(r)) missing.push(ratioOutputName(r));
  const ratioNames = [...new Set(ratios.map(ratioOutputName))];
  const columns = setup.ratio_mode === 'only' && ratioNames.length ? ratioNames : [...new Set([...elements, ...ratioNames])];
  const definition = { ...setup, elemental_columns: elements, ratios };
  if (!setupTransforms(definition)) return { columns, elements, missing };
  return { columns, elements, missing, transformation: definition };
}

/** One-line summary for the Data Manager, e.g. "12 of 33 elements · Log10". */
export function setupSummary(setup: TransformationDefinition | null, allElements: readonly string[]): string {
  if (!setup) return `All ${allElements.length} elements · no transform`;
  const used = setup.elemental_columns.filter((c) => allElements.includes(c)).length;
  const parts = [`${used} of ${allElements.length} elements`];
  if (setup.imputation_method !== 'none') parts.push(imputationSummary(setup));
  if (setup.ratios.length) parts.push(ratioSummary(setup.ratios.length, setup.ratio_mode));
  parts.push(setup.transform_method === 'none' ? 'no transform' : transformLabel(setup.transform_method));
  return parts.join(' · ');
}

/** "2 ratios appended" / "1 ratio only". */
export function ratioSummary(count: number, mode: RatioMode): string {
  return `${count} ${count === 1 ? 'ratio' : 'ratios'} ${mode === 'only' ? 'only' : 'appended'}`;
}

/** Union of measured columns across groups, in first-seen order. */
export function unionElements(lists: readonly (readonly string[])[]): string[] {
  const out: string[] = [];
  for (const list of lists) for (const c of list) if (!out.includes(c)) out.push(c);
  return out;
}
