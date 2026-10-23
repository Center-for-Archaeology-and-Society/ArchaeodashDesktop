/**
 * App-wide activity tracking: every backend call made through the transport
 * is counted here, so the top bar can show a spinner and what is running
 * (loading data, computing a view, saving…) for any process, without each
 * page wiring its own indicator. Calls that wait on the user (file pickers,
 * save dialogs) and quick bookkeeping calls are not tracked.
 */
import { useEffect, useState, useSyncExternalStore } from 'react';

/**
 * Friendly labels, first match wins. Keys are Tauri command names or, for
 * HTTP, `METHOD /path`.
 */
const LABELS: readonly [RegExp, string | null][] = [
  // Waiting on the user or trivial: never shown.
  [/open_project|pick_and_upload_source|save_export_file|app_info|current_project|recent_project|preferences|watch_analysis_job|stop_analysis_job_watch|get_analysis_job|list_transformations|load_transformation|^GET \/api\/v1\/(transformations|preferences|projects|quota)|\/jobs\/|\/auth\//, null],
  [/group_rows|groups\/rows/, 'Loading group data'],
  [/scan_group_candidates|validate_group_file|groups\/validate|\/api\/v1\/groups$/, 'Reading project files'],
  [/open_recent_project/, 'Opening project'],
  [/batch_transfer_units|transfer_units|transfer-units/, 'Assigning units to groups'],
  [/merge_groups|groups\/merge/, 'Merging groups'],
  [/duplicate_group|groups\/duplicate/, 'Duplicating group'],
  [/delete_group/, 'Deleting group'],
  [/patch_descriptive_values|descriptive-values/, 'Saving table edits'],
  [/open_import_preview|imports\/preview|upload_source_file|\/files/, 'Reading source file'],
  [/commit_group_import|imports\/commit/, 'Importing groups'],
  [/explore_missing|missing-profile/, 'Computing missing values'],
  [/explore_histogram|explore\/histogram/, 'Computing histogram'],
  [/explore_crosstab|explore\/crosstab/, 'Computing crosstab'],
  [/explore_compositional|compositional-profile/, 'Computing profile plot'],
  [/ordination_pca|ordination\/pca/, 'Computing PCA'],
  [/ordination_umap|ordination\/umap/, 'Computing UMAP'],
  [/ordination_lda|ordination\/lda/, 'Computing LDA'],
  [/cluster_diagnostics|cluster\/diagnostics/, 'Running cluster diagnostics'],
  [/cluster_fit|cluster\/fit|submit_analysis_job|\/api\/v1\/jobs$/, 'Running analysis'],
  [/membership_probabilities|membership\//, 'Computing membership probabilities'],
  [/euclidean_matches|euclidean\//, 'Finding Euclidean matches'],
  [/apply_transformation|transformations\/apply|batch_ratio|ratios\/batch/, 'Transforming values'],
  [/transformation/, 'Saving analysis setup'],
  [/export_|\/exports\//, 'Preparing export'],
];

/** The label for a command or request path; `null` = do not track. */
export function activityLabel(nameOrPath: string): string | null {
  for (const [pattern, label] of LABELS) if (pattern.test(nameOrPath)) return label;
  return 'Working';
}

export interface ActivitySnapshot {
  readonly count: number;
  /** Label of the most recently started task still running. */
  readonly label: string;
}

const IDLE: ActivitySnapshot = { count: 0, label: '' };

export class ActivityTracker {
  private readonly running = new Map<number, string>();
  private readonly listeners = new Set<() => void>();
  private nextId = 0;
  private current: ActivitySnapshot = IDLE;

  /** Counts `promise` as running work under `label` until it settles. */
  track<T>(label: string | null, promise: Promise<T>): Promise<T> {
    if (label === null) return promise;
    const id = this.nextId++;
    this.running.set(id, label);
    this.publish();
    const done = () => {
      this.running.delete(id);
      this.publish();
    };
    promise.then(done, done);
    return promise;
  }

  snapshot = (): ActivitySnapshot => this.current;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private publish(): void {
    const labels = [...this.running.values()];
    this.current = labels.length === 0 ? IDLE : { count: labels.length, label: labels[labels.length - 1]! };
    for (const listener of this.listeners) listener();
  }
}

export const activity = new ActivityTracker();

/**
 * Current activity, shown only after it has lasted `delayMs` (so instant
 * calls do not flash) and kept briefly after it ends (so back-to-back calls
 * do not flicker).
 */
export function useActivity(tracker: ActivityTracker = activity, delayMs = 250): ActivitySnapshot {
  const live = useSyncExternalStore(tracker.subscribe, tracker.snapshot, () => IDLE);
  const [shown, setShown] = useState<ActivitySnapshot>(IDLE);
  useEffect(() => {
    if (live.count > 0 && shown.count > 0) {
      setShown(live);
      return;
    }
    const timer = setTimeout(() => setShown(live), live.count > 0 ? delayMs : 150);
    return () => clearTimeout(timer);
  }, [live, shown.count, delayMs]);
  return shown;
}
