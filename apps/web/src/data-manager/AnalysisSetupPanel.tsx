/**
 * Data Manager section for the project's Analysis setup: a one-line summary
 * and a Configure… dialog that chooses the elements every analysis and plot
 * uses and the transform applied first.
 */
import { useEffect, useId, useState, type FormEvent, type ReactElement } from 'react';
import type { TransformationDefinition, TransformMethod } from '@archaeodash/contracts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { TRANSFORM_CHOICES, newSetup, setupSummary } from '../analysis-setup/setup.ts';
import { ImportDialog } from './ImportDialog.tsx';

export interface AnalysisSetupPanelProps {
  /** Measured columns across the project's group files. */
  readonly elements: readonly string[];
}

export function AnalysisSetupPanel({ elements }: AnalysisSetupPanelProps): ReactElement | null {
  const { setup, loaded, save, reset } = useAnalysisSetup();
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState('');
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
    if (!selected.length) return;
    const base = setup ?? newSetup(selected);
    void run(() => onSave({ ...base, elemental_columns: selected, transform_method: method }));
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
        <legend>Transform applied before analysis</legend>
        {TRANSFORM_CHOICES.map((choice) => (
          <label key={choice.value} className="setup-choice">
            <input type="radio" name="setup-transform" value={choice.value} checked={method === choice.value} disabled={busy} onChange={() => setMethod(choice.value)} />
            <span><strong>{choice.label}</strong> — {choice.hint}</span>
          </label>
        ))}
        {(method === 'log' || method === 'log10') && (
          <p className="setup-note">Zeros, negative and missing values have no logarithm; they become 0, as in the original ArchaeoDash. Values are rounded to three decimals.</p>
        )}
      </fieldset>
      <p className="setup-note">Group files keep their measured values; the setup is applied each time an analysis runs.</p>
      {error && <p role="alert">{error}</p>}
      <div className="setup-actions">
        {onReset && <button type="button" className="fm-tool" disabled={busy} onClick={() => void run(onReset)}>Reset to defaults</button>}
        <span className="setup-spacer" />
        <button type="button" className="fm-tool" disabled={busy} onClick={onCancel}>Cancel</button>
        <button type="submit" className="fm-tool fm-tool-primary" disabled={busy || !selected.length}>{busy ? 'Saving…' : 'Save'}</button>
      </div>
    </form>
  );
}
