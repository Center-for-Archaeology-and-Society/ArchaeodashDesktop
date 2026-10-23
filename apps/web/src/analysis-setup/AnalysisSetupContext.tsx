/**
 * Shares the project's Analysis setup with the Data Manager (which edits it)
 * and every analysis page (which follow it). Without a provider — as in page
 * tests — pages see no setup and behave as before: all measured elements,
 * no transform.
 */
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactElement, type ReactNode } from 'react';
import type { TransformationDefinition, TransformationsService } from '@archaeodash/client';
import { ANALYSIS_SETUP_NAME } from './setup.ts';

export interface AnalysisSetupState {
  readonly setup: TransformationDefinition | null;
  /** False while the saved setup loads; pages wait so they compute once. */
  readonly loaded: boolean;
  readonly error: string;
  /** Saves (replaces) the setup. Absent without a provider. */
  readonly save?: (setup: TransformationDefinition) => Promise<void>;
  /** Removes the setup, returning to all elements with no transform. */
  readonly reset?: () => Promise<void>;
}

const AnalysisSetupContext = createContext<AnalysisSetupState>({ setup: null, loaded: true, error: '' });

export function useAnalysisSetup(): AnalysisSetupState {
  return useContext(AnalysisSetupContext);
}

export interface AnalysisSetupProviderProps {
  readonly service: Pick<TransformationsService, 'list' | 'load' | 'save' | 'remove'>;
  /** Changes when another project opens, so its own setup is loaded. */
  readonly generation?: number | string | null;
  readonly children: ReactNode;
}

export function AnalysisSetupProvider({ service, generation, children }: AnalysisSetupProviderProps): ReactElement {
  const [setup, setSetup] = useState<TransformationDefinition | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    let active = true;
    setLoaded(false);
    setError('');
    (async () => {
      const list = await service.list();
      if (!list.transformations.some((t) => t.name === ANALYSIS_SETUP_NAME)) return null;
      return service.load(ANALYSIS_SETUP_NAME);
    })()
      .then((found) => { if (active) setSetup(found); })
      .catch((cause: unknown) => {
        if (!active) return;
        // No project open (or no catalog yet): analyses fall back to defaults.
        setSetup(null);
        setError(cause instanceof Error ? cause.message : String(cause));
      })
      .finally(() => { if (active) setLoaded(true); });
    return () => { active = false; };
  }, [service, generation]);

  const save = useCallback(async (next: TransformationDefinition) => {
    const saved = await service.save({ definition: { ...next, name: ANALYSIS_SETUP_NAME } });
    setSetup(saved.definition);
    setError('');
  }, [service]);

  const reset = useCallback(async () => {
    await service.remove(ANALYSIS_SETUP_NAME);
    setSetup(null);
  }, [service]);

  const value = useMemo<AnalysisSetupState>(() => ({ setup, loaded, error, save, reset }), [setup, loaded, error, save, reset]);
  return <AnalysisSetupContext.Provider value={value}>{children}</AnalysisSetupContext.Provider>;
}
