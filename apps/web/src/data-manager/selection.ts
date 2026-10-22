/**
 * File-manager selection rules (pure, unit-tested): plain click selects one,
 * Ctrl/Cmd-click toggles, Shift-click selects the range from the anchor.
 */
export interface SelectionState {
  readonly selected: readonly string[];
  readonly anchor: string | null;
}

export interface SelectModifiers {
  readonly toggle?: boolean;
  readonly range?: boolean;
}

export function applySelection(
  order: readonly string[],
  state: SelectionState,
  target: string,
  { toggle = false, range = false }: SelectModifiers = {},
): SelectionState {
  if (range && state.anchor !== null && order.includes(state.anchor)) {
    const from = order.indexOf(state.anchor);
    const to = order.indexOf(target);
    if (to < 0) return state;
    const [lo, hi] = from <= to ? [from, to] : [to, from];
    const span = order.slice(lo, hi + 1);
    const selected = toggle ? unique([...state.selected, ...span]) : span;
    return { selected: sortByOrder(order, selected), anchor: state.anchor };
  }
  if (toggle) {
    const selected = state.selected.includes(target)
      ? state.selected.filter((path) => path !== target)
      : sortByOrder(order, [...state.selected, target]);
    return { selected, anchor: target };
  }
  return { selected: [target], anchor: target };
}

/** Drop selected paths that no longer exist (after a rescan). */
export function pruneSelection(order: readonly string[], state: SelectionState): SelectionState {
  const selected = state.selected.filter((path) => order.includes(path));
  const anchor = state.anchor !== null && order.includes(state.anchor) ? state.anchor : null;
  return selected.length === state.selected.length && anchor === state.anchor ? state : { selected, anchor };
}

function unique(items: readonly string[]): string[] {
  return [...new Set(items)];
}

function sortByOrder(order: readonly string[], items: readonly string[]): string[] {
  return unique(items).sort((a, b) => order.indexOf(a) - order.indexOf(b));
}
