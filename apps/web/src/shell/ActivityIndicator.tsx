/**
 * Top-bar loading indicator for any backend work in progress (see
 * activity.ts): a labelled spinner chip and a thin animated bar along the
 * bottom of the navigation.
 */
import type { ReactElement } from 'react';
import { activity as defaultTracker, useActivity, type ActivityTracker } from '../activity.ts';
import { Spinner } from '../ui/Busy.tsx';

export function ActivityIndicator({ tracker = defaultTracker }: { tracker?: ActivityTracker }): ReactElement | null {
  const { count, label } = useActivity(tracker);
  if (count === 0) return null;
  const more = count > 1 ? ` (+${count - 1} more)` : '';
  return (
    <>
      <span className="activity-indicator" role="status" title={`${count} task${count === 1 ? '' : 's'} running`}>
        <Spinner />
        {`${label}…${more}`}
      </span>
      <span className="activity-bar" aria-hidden="true" />
    </>
  );
}
