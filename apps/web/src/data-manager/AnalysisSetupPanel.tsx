/**
 * Data Manager section for the project's Analysis setup: a one-line summary
 * and a Configure… dialog that chooses the elements every analysis and plot
 * uses and the transform applied first.
 */
import { useEffect, useId, useState, type FormEvent, type ReactElement } from 'react';
import type { GroupsService, OrdinationService } from '@archaeodash/client';
import type { ImputationMethod, OrdinationKind, TransformationDefinition, TransformMethod } from '@archaeodash/contracts';
import { ORDINATION_LABELS, planPrecompute, precomputeStatusText, type PrecomputeState } from '../analysis-setup/precompute.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { DEFAULT_IMPUTATION_SEED, IMPUTATION_CHOICES, TRANSFORM_CHOICES, newSetup, setupSummary } from '../analysis-setup/setup.ts';
import { useGroupInclusion } from '../datasets/GroupInclusion.tsx';
import { ImportDialog } from './ImportDialog.tsx';

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
  const { setup, loaded, save, reset } = useAnalysisSetup();
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState('');
  // Precompute the groups the pages analyze, so their requests hit the cache.
  const excluded = useGroupInclusion()?.excluded;
  const precompute = usePrecompute(setup, groups, ordination, dataKey, excluded);
  const headingId = useId();
  if (!save) return null;
  return (
    <section className="fm-setup" aria-labelledby={headingId}>
      <div className="fm-setup-head">
        <h3 id={headingId}>Analysis setup</h3>
        <button type="button" className="fm-tool" disabled={!loaded || elements.length === 0} onClick={() => { setStatus(''); setOpen(true); }}>
          Configure…
        </button>
      </div>
      <p className="fm-setup-summary">{loaded ? setupSummary(setup, elements) : 'Loading…'}</p>
      {precompute.length > 0 && (
        <ul className="fm-setup-precompute" aria-label="Precomputed ordinations">
          {precompute.map(([kind, state]) => (
            <li key={kind} className={typeof state === 'string' ? `is-${state}` : 'failed' in state ? 'is-failed' : 'is-skipped'}>
              {precomputeStatusText(kind, state)}
            </li>
          ))}
        </ul>
      )}
      {elements.length === 0 && loaded && <p className="fm-setup-hint">Import data to choose elements.</p>}
      {status && <p role="status" className="fm-setup-hint">{status}</p>}
      {open && (
        <ImportDialog title="Analysis setup" onClose={() => setOpen(false)}>
          <AnalysisSetupForm
            elements={elements}
            setup={setup}
            onSave={async (next) => { await save(next); setOpen(false); setStatus('Analysis setup saved. Analyses now use it.'); }}
            onReset={setup && reset ? async () => { await reset(); setOpen(false); setStatus('Analysis setup reset to all elements, no transform.'); } : undefined}
            onCancel={() => setOpen(false)}
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
      const candidates = (await groups.scan()).filter((c) => !excluded?.has(c.path));
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
  }, [setup, groups, ordination, dataKey, excluded]);
  return states;
}

export interface AnalysisSetupFormProps {
  readonly elements: readonly string[];
  readonly setup: TransformationDefinition | null;
  readonly onSave: (setup: TransformationDefinition) => Promise<void>;
  readonly onReset?: () => Promise<void>;
  readonly onCancel: () => void;
}

export function AnalysisSetupForm({ elements, setup, onSave, onReset, onCancel }: AnalysisSetupFormProps): ReactElement {
  const initialElements = setup ? elements.filter((e) => setup.elemental_columns.includes(e)) : [...elements];
  const [selected, setSelected] = useState<string[]>(initialElements);
  const [method, setMethod] = useState<TransformMethod>(setup?.transform_method ?? 'none');
  const [imputation, setImputation] = useState<ImputationMethod>(setup?.imputation_method ?? 'none');
  const [seed, setSeed] = useState(String(setup?.imputation_seed ?? DEFAULT_IMPUTATION_SEED));
  const [precompute, setPrecompute] = useState<OrdinationKind[]>(setup?.precompute ?? []);
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
        <button type="submit" className="fm-tool fm-tool-primary" disabled={busy || !selected.length || !seedValid}>{busy ? 'Saving…' : 'Save'}</button>
      </div>
    </form>
  );
}
