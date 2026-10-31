/**
 * The shared "Sample IDs" setting: one choice for every analysis page
 * (Explore, Ordination, Visualize, Cluster, Probabilities, Euclidean),
 * remembered in this browser's local storage. Each page shows the picker
 * next to its Dataset picker, since the selected groups decide whether
 * their IDs agree.
 */
import { useMemo, useSyncExternalStore, type ReactElement } from 'react';
import {
  ROWID_COLUMN, autoIdChoice, idChoiceLabel, idNotice, resolveIdChoice, sampleIds, summarizeIds,
  type IdFile, type IdPreference, type IdSummary, type SampleIds,
} from './sample-ids.ts';
import type { GroupRowDto } from '@archaeodash/client';

const STORAGE_KEY = 'archaeodash.sampleIds';

function isPreference(value: unknown): value is IdPreference {
  return value === 'auto' || value === 'visible' || value === 'rowid';
}

function readPreference(): IdPreference {
  try {
    const stored = globalThis.localStorage?.getItem(STORAGE_KEY);
    return isPreference(stored) ? stored : 'auto';
  } catch {
    return 'auto';
  }
}

let preference: IdPreference = readPreference();
const listeners = new Set<() => void>();

export function setIdPreference(next: IdPreference): void {
  if (next === preference) return;
  preference = next;
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, next);
  } catch {
    /* storage blocked: the choice still holds this session */
  }
  for (const listener of listeners) listener();
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};
const snapshot = () => preference;

export function useIdPreference(): IdPreference {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

export interface SampleIdState extends SampleIds {
  readonly preference: IdPreference;
  readonly summary: IdSummary;
  readonly notice: string;
}

/** The shared choice applied to the loaded files and their combined rows. */
export function useSampleIds(
  files: readonly IdFile[],
  combined: { readonly rows: readonly GroupRowDto[]; readonly groupOf: readonly string[] },
): SampleIdState {
  const current = useIdPreference();
  const summary = useMemo(() => summarizeIds(files), [files]);
  return useMemo(() => {
    const ids = sampleIds(combined, summary, resolveIdChoice(current, summary));
    return { ...ids, preference: current, summary, notice: idNotice(current, summary, ids) };
  }, [combined, summary, current]);
}

/** "Sample IDs" select with a short note when the IDs need attention. */
export function SampleIdPicker({ ids, disabled }: { ids: SampleIdState; disabled?: boolean }): ReactElement {
  return (
    <span className="sample-id-picker">
      <label className="field field-inline">
        Sample IDs
        <select value={ids.preference} disabled={disabled} onChange={(e) => setIdPreference(e.target.value as IdPreference)}>
          <option value="auto">{`Automatic: ${idChoiceLabel(ids.summary, autoIdChoice(ids.summary))}`}</option>
          <option value="visible">{idChoiceLabel(ids.summary, 'visible')}</option>
          <option value="rowid">{ROWID_COLUMN}</option>
        </select>
      </label>
      {ids.notice && <span className="field-hint" role="note">{ids.notice}</span>}
    </span>
  );
}
