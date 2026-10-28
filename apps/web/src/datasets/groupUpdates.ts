/**
 * Helpers around the group-change signal (data-manager/events.ts): a hook
 * for the "sample move under way" hold, a value frozen while it lasts, and
 * one shared project scan for every page that rescans after the same change.
 */
import { useRef, useSyncExternalStore } from 'react';
import type { GroupCandidate } from '@archaeodash/client';
import { groupUpdatesHeld, groupsChangeEpoch, onGroupUpdatesHeld } from '../data-manager/events.ts';

const subscribe = (onChange: () => void) => onGroupUpdatesHeld(onChange);

/** True while a sample move and the refresh after it are under way. */
export function useGroupUpdatesHeld(): boolean {
  return useSyncExternalStore(subscribe, groupUpdatesHeld, () => false);
}

/** `value`, except while `held`: then the value it had when the hold began. */
export function useHeldValue<T>(value: T, held: boolean): T {
  const kept = useRef(value);
  if (!held) kept.current = value;
  return kept.current;
}

interface InFlight {
  readonly epoch: number;
  readonly promise: Promise<GroupCandidate[]>;
}

const inFlight = new WeakMap<object, InFlight>();

/**
 * `service.scan()`, shared by callers in the same change epoch: after one
 * change notice, every page and the Data Manager get one scan instead of one
 * each. A scan that started before the latest notice is never reused.
 */
export function sharedScan(service: { scan: () => Promise<GroupCandidate[]> }): Promise<GroupCandidate[]> {
  const epoch = groupsChangeEpoch();
  const current = inFlight.get(service);
  if (current && current.epoch === epoch) return current.promise;
  const promise = service.scan().finally(() => {
    if (inFlight.get(service)?.promise === promise) inFlight.delete(service);
  });
  inFlight.set(service, { epoch, promise });
  return promise;
}
