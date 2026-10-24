/**
 * Dropdown with checkboxes for picking several values of one metadata field
 * (the legacy visualization filter's multi-select). An empty choice means
 * "all values".
 */
import { useEffect, useRef, useState, type ReactElement } from 'react';

export function ValueMultiSelect({
  values,
  selected,
  onChange,
  label,
}: {
  values: readonly string[];
  selected: ReadonlySet<string>;
  onChange: (next: Set<string>) => void;
  /** Accessible name prefix, e.g. "Filter values". */
  label: string;
}): ReactElement {
  const [open, setOpen] = useState(false);
  const holder = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (holder.current && !holder.current.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);
  const chosen = values.filter((v) => selected.has(v));
  const summary =
    chosen.length === 0 ? 'All values' : chosen.length <= 3 ? chosen.join(', ') : `${chosen.length} values`;
  const toggle = (value: string, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(value);
    else next.delete(value);
    onChange(next);
  };
  return (
    <div className="multiselect" ref={holder}>
      <button
        type="button"
        className="multiselect-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`${label}: ${summary}`}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') setOpen(false);
        }}
      >
        <span className="multiselect-summary">{summary}</span>
        <svg className="multiselect-chevron" viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2.5 4.5 6 8l3.5-3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
      </button>
      {open && (
        <div
          className="multiselect-menu"
          role="listbox"
          aria-multiselectable="true"
          onKeyDown={(e) => {
            if (e.key === 'Escape') setOpen(false);
          }}
        >
          <div className="multiselect-actions">
            <button type="button" className="btn btn-sm" onClick={() => onChange(new Set(values))}>
              All
            </button>
            <button type="button" className="btn btn-sm" onClick={() => onChange(new Set())}>
              Clear
            </button>
          </div>
          {values.map((v) => (
            <label key={v} className="multiselect-option" role="option" aria-selected={selected.has(v)}>
              <input type="checkbox" checked={selected.has(v)} onChange={(e) => toggle(v, e.target.checked)} /> {v}
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
