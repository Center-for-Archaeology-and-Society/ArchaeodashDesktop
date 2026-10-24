/**
 * Data Manager section for the project's Analysis setup: a one-line summary,
 * a Configure… dialog that chooses the elements every analysis and plot
 * uses, ratio variables, and the imputation and transform applied first, and
 * named setups that can be saved, loaded into the active setup, or deleted.
 */
import { useEffect, useId, useState, type FormEvent, type ReactElement } from 'react';
import type { AppliedTransformation, GroupsService, OrdinationService } from '@archaeodash/client';
import type {
  ImputationMethod,
  OrdinationKind,
  RatioMode,
  RatioSpecDto,
  TransformationDefinition,
  TransformationSummary,
  TransformMethod,
} from '@archaeodash/contracts';
import { ORDINATION_LABELS, planPrecompute, precomputeStatusText, type PrecomputeState } from '../analysis-setup/precompute.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import {
  DEFAULT_IMPUTATION_SEED,
  IMPUTATION_CHOICES,
  TRANSFORM_CHOICES,
  imputationLabel,
  mergeRatios,
  newSetup,
  ratioCombinations,
  ratioOutputName,
  setupNameProblem,
  setupStorageKey,
  setupSummary,
  transformLabel,
} from '../analysis-setup/setup.ts';
import { useGroupInclusion } from '../datasets/GroupInclusion.tsx';
import { activeFolderAmong, folderOf } from '../datasets/folders.ts';
import { ImportDialog } from './ImportDialog.tsx';
import { BusyLabel, Spinner } from '../ui/Busy.tsx';

export interface AnalysisSetupPanelProps {
  /** Measured columns across the project's group files. */
  readonly elements: readonly string[];
  /** Services for background precompute; without them nothing is precomputed. */
  readonly groups?: Pick<GroupsService, 'scan'>;
  readonly ordination?: OrdinationService;
  /** Changes whenever group files change, so precomputed results refresh. */
  readonly dataKey?: string;
}

export function AnalysisSetupPanel({ elements, groups, ordination, dataKey = '' }: AnalysisSetupPanelProps): ReactElement | null {
  const { setup, loaded, save, reset, saved, saveAs, loadSaved, deleteSaved, preview } = useAnalysisSetup();
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState('');
  // Precompute the groups the pages analyze, so their requests hit the cache.
  const inclusion = useGroupInclusion();
  const precompute = usePrecompute(setup, groups, ordination, dataKey, inclusion?.excluded, inclusion?.activeFolder);
  const headingId = useId();
  if (!save) return null;
  // Ratio preview: raw ratios over the groups the pages analyze.
  const onPreview = preview && groups
    ? async (ratios: RatioSpecDto[]): Promise<AppliedTransformation | null> => {
      const scanned = (await groups.scan()).filter((c) => c.ready && c.group);
      const folder = activeFolderAmong(scanned.map((c) => c.path), inclusion?.activeFolder);
      const paths = scanned.filter((c) => (folder === null || folderOf(c.path) === folder) && !inclusion?.excluded?.has(c.path)).map((c) => c.path);
      const [path, ...extra] = paths;
      if (!path) return null;
      return preview({ path, extra_paths: extra, definition: ratioPreviewDefinition(ratios) });
    }
    : undefined;
  return (
    <section className="fm-setup" aria-labelledby={headingId}>
      <div className="fm-setup-head">
        <h3 id={headingId}>Analysis setup</h3>
        <button type="button" className="fm-tool" disabled={!loaded || elements.length === 0} onClick={() => { setStatus(''); setOpen(true); }}>
          Configure…
        </button>
      </div>
      <p className="fm-setup-summary">{loaded ? setupSummary(setup, elements) : <><Spinner />Loading…</>}</p>
      {precompute.length > 0 && (
        <ul className="fm-setup-precompute" aria-label="Precomputed ordinations">
          {precompute.map(([kind, state]) => (
            <li key={kind} className={typeof state === 'string' ? `is-${state}` : 'failed' in state ? 'is-failed' : 'is-skipped'}>
              {state === 'computing' && <Spinner />}
              {precomputeStatusText(kind, state)}
            </li>
          ))}
        </ul>
      )}
      {elements.length === 0 && loaded && <p className="fm-setup-hint">Import data to choose elements.</p>}
      {status && <p role="status" className="fm-setup-hint">{status}</p>}
      {loaded && saveAs && loadSaved && deleteSaved && (
        <SavedSetups
          saved={saved ?? []}
          disabled={elements.length === 0}
          onSaveAs={async (name) => { await saveAs(name, setup ?? newSetup(elements)); setStatus(`Saved the active setup as “${name.trim()}”.`); }}
          onLoad={async (name) => { await loadSaved(name); setStatus(`Loaded “${name}” into the active setup. Analyses now use it.`); }}
          onDelete={async (name) => { await deleteSaved(name); setStatus(`Deleted saved setup “${name}”.`); }}
        />
      )}
      {open && (
        <ImportDialog title="Analysis setup" onClose={() => setOpen(false)}>
          <AnalysisSetupForm
            elements={elements}
            setup={setup}
            onSave={async (next) => { await save(next); setOpen(false); setStatus('Analysis setup saved. Analyses now use it.'); }}
            onReset={setup && reset ? async () => { await reset(); setOpen(false); setStatus('Analysis setup reset to all elements, no transform.'); } : undefined}
            onCancel={() => setOpen(false)}
            onPreview={onPreview}
          />
        </ImportDialog>
      )}
    </section>
  );
}

/**
 * Runs the setup's precompute choices whenever the setup or the group files
 * change; results land in the backend's ordination cache.
 */
function usePrecompute(
  setup: TransformationDefinition | null,
  groups: Pick<GroupsService, 'scan'> | undefined,
  ordination: OrdinationService | undefined,
  dataKey: string,
  excluded: ReadonlySet<string> | undefined,
  activeFolder: string | null | undefined,
): [OrdinationKind, PrecomputeState][] {
  const [states, setStates] = useState<[OrdinationKind, PrecomputeState][]>([]);
  useEffect(() => {
    if (!setup?.precompute?.length || !groups || !ordination) {
      setStates([]);
      return;
    }
    let cancelled = false;
    const update = (kind: OrdinationKind, state: PrecomputeState) => {
      if (!cancelled) setStates((current) => current.map(([k, s]) => [k, k === kind ? state : s]));
    };
    void (async () => {
      const scanned = (await groups.scan()).filter((c) => c.ready && c.group);
      // Same groups the pages analyze: the active folder, minus exclusions.
      const folder = activeFolderAmong(scanned.map((c) => c.path), activeFolder);
      const candidates = scanned.filter((c) => (folder === null || folderOf(c.path) === folder) && !excluded?.has(c.path));
      const plan = planPrecompute(setup, candidates);
      if (cancelled) return;
      setStates([
        ...plan.jobs.map((job): [OrdinationKind, PrecomputeState] => [job.kind, 'waiting']),
        ...plan.skipped.map((s): [OrdinationKind, PrecomputeState] => [s.kind, { skipped: s.reason }]),
      ]);
      for (const job of plan.jobs) {
        if (cancelled) return;
        update(job.kind, 'computing');
        try {
          await job.run(ordination);
          update(job.kind, 'ready');
        } catch (cause) {
          update(job.kind, { failed: cause instanceof Error ? cause.message : String(cause) });
        }
      }
    })().catch(() => { if (!cancelled) setStates([]); });
    return () => { cancelled = true; };
  }, [setup, groups, ordination, dataKey, excluded, activeFolder]);
  return states;
}

export interface AnalysisSetupFormProps {
  readonly elements: readonly string[];
  readonly setup: TransformationDefinition | null;
  readonly onSave: (setup: TransformationDefinition) => Promise<void>;
  readonly onReset?: () => Promise<void>;
  readonly onCancel: () => void;
  /** Computes raw ratio values for a preview; null when there are no groups. */
  readonly onPreview?: (ratios: RatioSpecDto[]) => Promise<AppliedTransformation | null>;
}

export function AnalysisSetupForm({ elements, setup, onSave, onReset, onCancel, onPreview }: AnalysisSetupFormProps): ReactElement {
  const initialElements = setup ? elements.filter((e) => setup.elemental_columns.includes(e)) : [...elements];
  const [selected, setSelected] = useState<string[]>(initialElements);
  const [method, setMethod] = useState<TransformMethod>(setup?.transform_method ?? 'none');
  const [imputation, setImputation] = useState<ImputationMethod>(setup?.imputation_method ?? 'none');
  const [seed, setSeed] = useState(String(setup?.imputation_seed ?? DEFAULT_IMPUTATION_SEED));
  const [precompute, setPrecompute] = useState<OrdinationKind[]>(setup?.precompute ?? []);
  const [ratios, setRatios] = useState<RatioSpecDto[]>(setup?.ratios ?? []);
  const [ratioMode, setRatioMode] = useState<RatioMode>(setup?.ratio_mode ?? 'append');
  const seedValue = Number(seed);
  const seedValid = imputation === 'none' || (seed.trim() !== '' && Number.isInteger(seedValue) && seedValue >= 0 && seedValue <= Number.MAX_SAFE_INTEGER);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  // Keep the selection inside the project's elements if files change meanwhile.
  useEffect(() => { setSelected((current) => current.filter((e) => elements.includes(e))); }, [elements]);

  const toggle = (element: string, on: boolean) =>
    setSelected((current) => (on ? elements.filter((e) => e === element || current.includes(e)) : current.filter((e) => e !== element)));

  const run = async (work: () => Promise<void>) => {
    setBusy(true); setError('');
    try { await work(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!selected.length || !seedValid) return;
    const base = setup ?? newSetup(selected);
    void run(() => onSave({
      ...base,
      elemental_columns: selected,
      transform_method: method,
      imputation_method: imputation,
      imputation_seed: imputation === 'none' ? null : seedValue,
      ratios,
      ratio_mode: ratioMode,
      precompute,
    }));
  };

  return (
    <form className="setup-form" onSubmit={submit}>
      <fieldset className="setup-elements">
        <legend>Elements used in analyses and plots ({selected.length} of {elements.length})</legend>
        <div className="setup-row">
          <button type="button" disabled={busy} onClick={() => setSelected([...elements])}>Select all</button>
          <button type="button" disabled={busy} onClick={() => setSelected([])}>Clear</button>
        </div>
        <div className="setup-element-grid">
          {elements.map((element) => (
            <label key={element}>
              <input type="checkbox" checked={selected.includes(element)} disabled={busy} onChange={(event) => toggle(element, event.target.checked)} /> {element}
            </label>
          ))}
        </div>
        {!selected.length && <p role="alert">Choose at least one element.</p>}
      </fieldset>
      <fieldset>
        <legend>Fill missing values (imputation)</legend>
        {IMPUTATION_CHOICES.map((choice) => (
          <label key={choice.value} className="setup-choice">
            <input type="radio" name="setup-imputation" value={choice.value} checked={imputation === choice.value} disabled={busy} onChange={() => setImputation(choice.value)} />
            <span><strong>{choice.label}</strong> — {choice.hint}</span>
          </label>
        ))}
        {imputation !== 'none' && (
          <label className="setup-seed">
            Seed
            <input type="number" min={0} step={1} value={seed} disabled={busy} onChange={(event) => setSeed(event.target.value)} aria-describedby="setup-seed-note" />
          </label>
        )}
        {imputation !== 'none' && (
          <p className="setup-note" id="setup-seed-note">
            Imputation runs before the transform, on the selected elements. The same seed gives the same imputed values; change it for a different draw.
          </p>
        )}
        {!seedValid && <p role="alert">Enter a whole-number seed of 0 or more.</p>}
      </fieldset>
      <RatioFieldset elements={elements} ratios={ratios} mode={ratioMode} busy={busy} onRatios={setRatios} onMode={setRatioMode} onPreview={onPreview} />
      <fieldset>
        <legend>Transform applied before analysis</legend>
        {TRANSFORM_CHOICES.map((choice) => (
          <label key={choice.value} className="setup-choice">
            <input type="radio" name="setup-transform" value={choice.value} checked={method === choice.value} disabled={busy} onChange={() => setMethod(choice.value)} />
            <span><strong>{choice.label}</strong> — {choice.hint}</span>
          </label>
        ))}
        {(method === 'log' || method === 'log10') && (
          <p className="setup-note">Zeros, negative and missing values have no logarithm; they become 0, as in the original ArchaeoDash (impute first to avoid zeros for missing values). Values are rounded to three decimals.</p>
        )}
      </fieldset>
      <fieldset>
        <legend>Precompute in the background</legend>
        <div className="setup-row">
          {(['pca', 'umap', 'lda'] as const).map((kind) => (
            <label key={kind}>
              <input
                type="checkbox"
                checked={precompute.includes(kind)}
                disabled={busy}
                onChange={(event) => setPrecompute((current) => (event.target.checked ? [...current.filter((k) => k !== kind), kind] : current.filter((k) => k !== kind)))}
              /> {ORDINATION_LABELS[kind]}
            </label>
          ))}
        </div>
        <p className="setup-note">
          Computed for all groups after you save and whenever group files change, then reused by Ordination and Visualize instead of
          computing on the fly. Results are kept in the project&apos;s disposable cache (.archaeodash/cache). LDA needs three or more groups.
        </p>
      </fieldset>
      <p className="setup-note">Group files keep their measured values; the setup is applied each time an analysis runs.</p>
      {error && <p role="alert">{error}</p>}
      <div className="setup-actions">
        {onReset && <button type="button" className="fm-tool" disabled={busy} onClick={() => void run(onReset)}>Reset to defaults</button>}
        <span className="setup-spacer" />
        <button type="button" className="fm-tool" disabled={busy} onClick={onCancel}>Cancel</button>
        <button type="submit" className="fm-tool fm-tool-primary" disabled={busy || !selected.length || !seedValid}><BusyLabel busy={busy} busyText="Saving…">Save</BusyLabel></button>
      </div>
    </form>
  );
}

/** Definition that returns the raw ratio values (no imputation or transform). */
export function ratioPreviewDefinition(ratios: readonly RatioSpecDto[]): TransformationDefinition {
  return { ...newSetup([]), name: 'ratio_preview', ratios: [...ratios], ratio_mode: 'only' };
}

/** Rows shown in the ratio preview. */
const PREVIEW_ROWS = 5;

interface RatioFieldsetProps {
  readonly elements: readonly string[];
  readonly ratios: readonly RatioSpecDto[];
  readonly mode: RatioMode;
  readonly busy: boolean;
  readonly onRatios: (ratios: RatioSpecDto[]) => void;
  readonly onMode: (mode: RatioMode) => void;
  readonly onPreview?: (ratios: RatioSpecDto[]) => Promise<AppliedTransformation | null>;
}

type PreviewState =
  | { readonly state: 'idle' | 'loading' }
  | { readonly state: 'error'; readonly message: string }
  | { readonly state: 'ready'; readonly result: AppliedTransformation | null };

/**
 * Ratio builder (legacy "Build Ratio Variables"): every chosen numerator
 * over every chosen denominator, an optional name for a single pair, the
 * current ratios with Remove, a preview of computed values, and whether
 * analyses use the ratios with the elements or alone.
 */
export function RatioFieldset({ elements, ratios, mode, busy, onRatios, onMode, onPreview }: RatioFieldsetProps): ReactElement {
  const [numerators, setNumerators] = useState<string[]>([]);
  const [denominators, setDenominators] = useState<string[]>([]);
  const [name, setName] = useState('');
  const [problem, setProblem] = useState('');
  const [preview, setPreview] = useState<PreviewState>({ state: 'idle' });
  const single = numerators.length === 1 && denominators.length === 1;
  const defaultName = single ? ratioOutputName({ numerator: numerators[0]!, denominator: denominators[0]! }) : '';
  const combos = ratioCombinations(numerators, denominators);
  const numeratorId = useId();
  const denominatorId = useId();
  const listSize = Math.min(6, Math.max(3, elements.length));

  // A changed ratio list makes the shown preview stale.
  useEffect(() => { setPreview({ state: 'idle' }); }, [ratios]);

  const add = () => {
    if (!numerators.length || !denominators.length) { setProblem('Choose at least one numerator and one denominator.'); return; }
    const added = ratioCombinations(numerators, denominators, single ? name : '');
    if (!added.length) { setProblem('Choose different elements for the numerator and denominator.'); return; }
    const clash = added.map(ratioOutputName).find((n) => elements.includes(n));
    if (clash) { setProblem(`“${clash}” is already an element name; choose another ratio name.`); return; }
    setProblem('');
    setName('');
    onRatios(mergeRatios(ratios, added));
  };

  const runPreview = () => {
    if (!onPreview) return;
    setPreview({ state: 'loading' });
    onPreview([...ratios])
      .then((result) => setPreview({ state: 'ready', result }))
      .catch((cause: unknown) => setPreview({ state: 'error', message: cause instanceof Error ? cause.message : String(cause) }));
  };

  const chosen = (select: HTMLSelectElement) => Array.from(select.selectedOptions, (o) => o.value);

  return (
    <fieldset className="setup-ratios">
      <legend>Ratio variables ({ratios.length})</legend>
      <p className="setup-note">
        Each ratio is numerator ÷ denominator of the measured (imputed) values, computed before the transform; it is missing where the
        denominator is missing or zero. Choose several numerators or denominators to add every combination.
      </p>
      <div className="setup-ratio-builder">
        <label htmlFor={numeratorId}>Numerator(s)</label>
        <label htmlFor={denominatorId}>Denominator(s)</label>
        <select id={numeratorId} multiple size={listSize} value={numerators} disabled={busy} onChange={(event) => setNumerators(chosen(event.target))}>
          {elements.map((e) => <option key={e} value={e}>{e}</option>)}
        </select>
        <select id={denominatorId} multiple size={listSize} value={denominators} disabled={busy} onChange={(event) => setDenominators(chosen(event.target))}>
          {elements.map((e) => <option key={e} value={e}>{e}</option>)}
        </select>
      </div>
      <div className="setup-row">
        <label className="setup-ratio-name">
          Name
          <input
            type="text"
            value={single ? name : ''}
            placeholder={single ? defaultName : 'numerator_denominator'}
            disabled={busy || !single}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <button type="button" disabled={busy || !numerators.length || !denominators.length} onClick={add}>
          {combos.length > 1 ? `Add ${combos.length} ratios` : 'Add ratio'}
        </button>
      </div>
      {problem && <p role="alert">{problem}</p>}
      {ratios.length > 0 && (
        <ul className="setup-ratio-list" aria-label="Ratio variables">
          {ratios.map((r) => {
            const output = ratioOutputName(r);
            return (
              <li key={output}>
                <span><strong>{output}</strong> = {r.numerator} ÷ {r.denominator}</span>
                <button type="button" disabled={busy} aria-label={`Remove ${output}`} onClick={() => onRatios(ratios.filter((x) => x !== r))}>Remove</button>
              </li>
            );
          })}
        </ul>
      )}
      {ratios.length > 0 && onPreview && (
        <div className="setup-ratio-preview">
          <button type="button" disabled={busy || preview.state === 'loading'} onClick={runPreview}>
            <BusyLabel busy={preview.state === 'loading'} busyText="Computing…">Preview values</BusyLabel>
          </button>
          {preview.state === 'error' && <p role="alert">{preview.message}</p>}
          {preview.state === 'ready' && !preview.result && <p className="setup-note">No group files to preview.</p>}
          {preview.state === 'ready' && preview.result && <RatioPreviewTable result={preview.result} />}
        </div>
      )}
      <div className="setup-row" role="radiogroup" aria-label="How analyses use ratios">
        <label className="setup-choice">
          <input type="radio" name="setup-ratio-mode" value="append" checked={mode === 'append'} disabled={busy} onChange={() => onMode('append')} />
          <span><strong>Append to selected elements</strong></span>
        </label>
        <label className="setup-choice">
          <input type="radio" name="setup-ratio-mode" value="only" checked={mode === 'only'} disabled={busy} onChange={() => onMode('only')} />
          <span><strong>Use ratios only</strong></span>
        </label>
      </div>
      {mode === 'only' && !ratios.length && <p className="setup-note">With no ratios, analyses use the selected elements.</p>}
    </fieldset>
  );
}

/** First rows of the computed (raw) ratio values. */
export function RatioPreviewTable({ result }: { readonly result: AppliedTransformation }): ReactElement {
  const rows = result.rows.slice(0, PREVIEW_ROWS);
  return (
    <div className="setup-ratio-table">
      <table>
        <caption>First {rows.length} of {result.rows.length} rows, before the transform</caption>
        <thead><tr>{result.columns.map((c) => <th key={c} scope="col">{c}</th>)}</tr></thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i}>{row.map((v, j) => <td key={j}>{v === null ? 'NA' : Number(v.toPrecision(4))}</td>)}</tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

interface SavedSetupsProps {
  readonly saved: readonly TransformationSummary[];
  readonly disabled: boolean;
  readonly onSaveAs: (name: string) => Promise<void>;
  readonly onLoad: (name: string) => Promise<void>;
  readonly onDelete: (name: string) => Promise<void>;
}

type Pending =
  | { readonly kind: 'idle' }
  | { readonly kind: 'naming' }
  | { readonly kind: 'replace'; readonly name: string }
  | { readonly kind: 'delete'; readonly name: string };

/**
 * Named setups (legacy saved transformations): save the active setup under
 * a name, load one into the active setup, or delete one. Replacing or
 * deleting asks first.
 */
export function SavedSetups({ saved, disabled, onSaveAs, onLoad, onDelete }: SavedSetupsProps): ReactElement {
  const [pending, setPending] = useState<Pending>({ kind: 'idle' });
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const headingId = useId();

  const run = async (work: () => Promise<void>) => {
    setBusy(true); setError('');
    try { await work(); setPending({ kind: 'idle' }); setName(''); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setBusy(false); }
  };

  const submitName = (event: FormEvent) => {
    event.preventDefault();
    const problem = setupNameProblem(name);
    if (problem) { setError(problem); return; }
    const existing = saved.find((s) => setupStorageKey(s.name) === setupStorageKey(name));
    if (existing) { setError(''); setPending({ kind: 'replace', name: existing.name }); return; }
    void run(() => onSaveAs(name));
  };

  return (
    <section className="setup-saved" aria-labelledby={headingId}>
      <div className="fm-setup-head">
        <h4 id={headingId}>Saved setups</h4>
        <button type="button" className="fm-tool" disabled={disabled || busy || pending.kind !== 'idle'} onClick={() => { setError(''); setPending({ kind: 'naming' }); }}>
          Save as…
        </button>
      </div>
      {(pending.kind === 'naming' || pending.kind === 'replace') && (
        <form className="setup-saved-form" onSubmit={submitName}>
          <label>
            Name
            <input type="text" value={name} maxLength={64} disabled={busy || pending.kind === 'replace'} onChange={(event) => setName(event.target.value)} />
          </label>
          {pending.kind === 'naming' ? (
            <span className="setup-saved-actions">
              <button type="submit" className="fm-tool fm-tool-primary" disabled={busy || !name.trim()}><BusyLabel busy={busy} busyText="Saving…">Save</BusyLabel></button>
              <button type="button" className="fm-tool" disabled={busy} onClick={() => { setPending({ kind: 'idle' }); setError(''); }}>Cancel</button>
            </span>
          ) : (
            <p role="alert" className="setup-confirm">
              A saved setup named “{pending.name}” exists. Replace it?{' '}
              <button type="button" className="fm-tool fm-tool-primary" disabled={busy} onClick={() => void run(() => onSaveAs(name))}>Replace</button>{' '}
              <button type="button" className="fm-tool" disabled={busy} onClick={() => setPending({ kind: 'naming' })}>Cancel</button>
            </p>
          )}
        </form>
      )}
      {error && <p role="alert" className="fm-setup-hint">{error}</p>}
      {saved.length === 0 ? (
        <p className="fm-setup-hint">No saved setups.</p>
      ) : (
        <ul className="setup-saved-list" aria-label="Saved setups">
          {saved.map((s) => (
            <li key={s.name}>
              <span className="setup-saved-name"><strong>{s.name}</strong> <small>{savedSummary(s)}</small></span>
              {pending.kind === 'delete' && pending.name === s.name ? (
                <span className="setup-confirm" role="alert">
                  Delete “{s.name}”?{' '}
                  <button type="button" className="fm-tool" disabled={busy} onClick={() => void run(() => onDelete(s.name))}>Delete</button>{' '}
                  <button type="button" className="fm-tool" disabled={busy} onClick={() => setPending({ kind: 'idle' })}>Keep</button>
                </span>
              ) : (
                <span className="setup-saved-actions">
                  <button type="button" className="fm-tool" disabled={busy || pending.kind !== 'idle'} onClick={() => void run(() => onLoad(s.name))}>Load</button>
                  <button type="button" className="fm-tool" disabled={busy || pending.kind !== 'idle'} onClick={() => { setError(''); setPending({ kind: 'delete', name: s.name }); }}>Delete</button>
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** Short description of a saved setup, e.g. "PMM imputation · 2 ratios · Log10". */
export function savedSummary(s: TransformationSummary): string {
  const parts: string[] = [];
  if (s.imputation_method !== 'none') parts.push(imputationLabel(s.imputation_method));
  if (s.ratio_count) parts.push(`${s.ratio_count} ${s.ratio_count === 1 ? 'ratio' : 'ratios'}`);
  parts.push(s.transform_method === 'none' ? 'no transform' : transformLabel(s.transform_method));
  return parts.join(' · ');
}
