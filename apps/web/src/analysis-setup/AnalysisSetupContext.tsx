/**
 * Shares the project's Analysis setup with the Data Manager (which edits it)
 * and every analysis page (which follow it). Without a provider — as in page
 * tests — pages see no setup and behave as before: all measured elements,
 * no transform.
 */
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from 'react';
import type {
  AppliedTransformation,
  ApplyTransformationRequest,
  TransformationDefinition,
  TransformationSummary,
  TransformationsService,
} from '@archaeodash/client';
import { ANALYSIS_SETUP_NAME, setupNameProblem, setupStorageKey } from './setup.ts';

export interface AnalysisSetupState {
  readonly setup: TransformationDefinition | null;
  /** False while the saved setup loads; pages wait so they compute once. */
  readonly loaded: boolean;
  readonly error: string;
  /** Saves (replaces) the setup. Absent without a provider. */
  readonly save?: (setup: TransformationDefinition) => Promise<void>;
  /** Removes the setup, returning to all elements with no transform. */
  readonly reset?: () => Promise<void>;
  /** Named setups saved in the project (summaries only; loaded on demand). */
  readonly saved?: readonly TransformationSummary[];
  /** Saves `setup` under `name`, replacing a saved setup with the same name. */
  readonly saveAs?: (name: string, setup: TransformationDefinition) => Promise<void>;
  /** Copies a named setup into the active setup. */
  readonly loadSaved?: (name: string) => Promise<void>;
  /** Deletes a named setup; the active setup is unchanged. */
  readonly deleteSaved?: (name: string) => Promise<void>;
  /** Applies a definition without saving it (for previews), when supported. */
  readonly preview?: (request: ApplyTransformationRequest) => Promise<AppliedTransformation>;
}

const AnalysisSetupContext = createContext<AnalysisSetupState>({ setup: null, loaded: true, error: '' });

export function useAnalysisSetup(): AnalysisSetupState {
  return useContext(AnalysisSetupContext);
}

export interface AnalysisSetupProviderProps {
  readonly service: Pick<TransformationsService, 'list' | 'load' | 'save' | 'remove'> & Partial<Pick<TransformationsService, 'apply'>>;
  /** Changes when another project opens, so its own setup is loaded. */
  readonly generation?: number | string | null;
  readonly children: ReactNode;
}

export function AnalysisSetupProvider({ service, generation, children }: AnalysisSetupProviderProps): ReactElement {
  const [setup, setSetup] = useState<TransformationDefinition | null>(null);
  const [saved, setSaved] = useState<readonly TransformationSummary[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState('');
  /**
   * Bumped when another project opens: responses to requests made for the
   * previous project are dropped instead of overwriting the new project's
   * setup (and a saved setup loaded from it is not written into the new one).
   */
  const epoch = useRef(0);

  useEffect(() => {
    let active = true;
    epoch.current += 1;
    // The previous project's setup must not show (or drive a precompute)
    // while this project's loads.
    setSetup(null);
    setSaved([]);
    setLoaded(false);
    setError('');
    (async () => {
      const list = await service.list();
      if (active) setSaved(namedSetups(list.transformations));
      if (!list.transformations.some((t) => t.name === ANALYSIS_SETUP_NAME)) return null;
      return service.load(ANALYSIS_SETUP_NAME);
    })()
      .then((found) => { if (active) setSetup(found); })
      .catch((cause: unknown) => {
        if (!active) return;
        // No project open (or no catalog yet): analyses fall back to defaults.
        setSetup(null);
        setSaved([]);
        setError(cause instanceof Error ? cause.message : String(cause));
      })
      .finally(() => { if (active) setLoaded(true); });
    return () => { active = false; };
  }, [service, generation]);

  const save = useCallback(async (next: TransformationDefinition) => {
    const at = epoch.current;
    const saved = await service.save({ definition: { ...next, name: ANALYSIS_SETUP_NAME } });
    if (at !== epoch.current) return;
    setSetup(saved.definition);
    setError('');
  }, [service]);

  const reset = useCallback(async () => {
    const at = epoch.current;
    await service.remove(ANALYSIS_SETUP_NAME);
    if (at === epoch.current) setSetup(null);
  }, [service]);

  const refreshSaved = useCallback(async () => {
    const at = epoch.current;
    const list = await service.list();
    if (at === epoch.current) setSaved(namedSetups(list.transformations));
  }, [service]);

  const saveAs = useCallback(async (name: string, next: TransformationDefinition) => {
    const problem = setupNameProblem(name);
    if (problem) throw new Error(problem);
    await service.save({ definition: { ...next, name: name.trim() } });
    await refreshSaved();
  }, [service, refreshSaved]);

  const loadSaved = useCallback(async (name: string) => {
    const at = epoch.current;
    const definition = await service.load(name);
    // Another project opened meanwhile: do not write this one's setup there.
    if (at !== epoch.current) return;
    await save(definition);
  }, [service, save]);

  const deleteSaved = useCallback(async (name: string) => {
    if (setupStorageKey(name) === ANALYSIS_SETUP_NAME) throw new Error('The active setup is removed with Reset to defaults.');
    await service.remove(name);
    await refreshSaved();
  }, [service, refreshSaved]);

  const apply = service.apply;
  const preview = useMemo(() => (apply ? (request: ApplyTransformationRequest) => apply.call(service, request) : undefined), [service, apply]);

  const value = useMemo<AnalysisSetupState>(
    () => ({ setup, loaded, error, save, reset, saved, saveAs, loadSaved, deleteSaved, preview }),
    [setup, loaded, error, save, reset, saved, saveAs, loadSaved, deleteSaved, preview],
  );
  return <AnalysisSetupContext.Provider value={value}>{children}</AnalysisSetupContext.Provider>;
}

/** Saved definitions other than the reserved active setup, by name. */
export function namedSetups(list: readonly TransformationSummary[]): TransformationSummary[] {
  return list.filter((t) => setupStorageKey(t.name) !== ANALYSIS_SETUP_NAME).sort((a, b) => a.name.localeCompare(b.name));
}
