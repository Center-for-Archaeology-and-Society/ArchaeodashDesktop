/**
 * Import progress: an indeterminate bar with the current step and elapsed
 * time, so a slow read or write never looks stuck. The backend does not
 * report percentages for imports, so the bar animates rather than fills.
 */
import { useEffect, useState, type ReactElement } from 'react';

export type ImportStep = 'uploading' | 'picking' | 'reading' | 'grouping' | 'importing';

/** User-facing label for a step; `groups` is the number of files to write. */
export function importStepLabel(step: ImportStep, groups = 0): string {
  switch (step) {
    case 'uploading': return 'Uploading the file…';
    case 'picking': return 'Selecting and copying the file into the project…';
    case 'reading': return 'Reading columns and rows…';
    case 'grouping': return 'Counting rows per group…';
    case 'importing': return groups > 0 ? `Writing ${groups} group file${groups === 1 ? '' : 's'}…` : 'Writing group files…';
  }
}

/** Whole seconds since `startedAt`, as shown under the bar. */
export function elapsedLabel(startedAt: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - startedAt) / 1000));
  return seconds < 60 ? `${seconds} s` : `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

export interface ImportProgressProps {
  readonly step: ImportStep;
  readonly startedAt: number;
  readonly groups?: number;
}

export function ImportProgress({ step, startedAt, groups = 0 }: ImportProgressProps): ReactElement {
  const [now, setNow] = useState(startedAt);
  useEffect(() => {
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [startedAt]);
  const label = importStepLabel(step, groups);
  return (
    <div className="import-progress" role="status">
      <div
        className="import-progress-track"
        role="progressbar"
        aria-label="Import progress"
        aria-valuetext={label}
        aria-busy="true"
      >
        <div className="import-progress-bar" />
      </div>
      <p className="import-progress-text">
        <span>{label}</span>
        <span className="import-progress-elapsed">{elapsedLabel(startedAt, now)}</span>
      </p>
    </div>
  );
}
