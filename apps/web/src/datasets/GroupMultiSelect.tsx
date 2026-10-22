/**
 * Dataset picker: a dropdown of the project's groups (by name) with
 * checkboxes, All / None shortcuts, and a summary of what is selected.
 */
import { useEffect, useRef, useState, type ReactElement } from 'react';

/** Dropdown with checkboxes: pick any subset of the project's group files. */
export function GroupMultiSelect({
  options,
  selected,
  onChange,
  disabled = false,
}: {
  options: readonly { path: string; name: string }[];
  selected: readonly string[];
  onChange: (paths: string[]) => void;
  disabled?: boolean;
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
  const names = options.filter((o) => selected.includes(o.path)).map((o) => o.name);
  const summary =
    names.length === 0
      ? 'No groups'
      : names.length === options.length && options.length > 1
        ? `All groups (${names.length})`
        : names.join(', ');
  const toggle = (path: string, on: boolean) =>
    onChange(options.map((o) => o.path).filter((p) => (p === path ? on : selected.includes(p))));
  return (
    <div className="multiselect" ref={holder}>
      <button
        type="button"
        className="multiselect-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Dataset: ${summary}`}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') setOpen(false);
        }}
      >
        <span className="multiselect-summary">{summary}</span> ▾
      </button>
      {open && !disabled && (
        <div
          className="multiselect-menu"
          role="listbox"
          aria-multiselectable="true"
          onKeyDown={(e) => {
            if (e.key === 'Escape') setOpen(false);
          }}
        >
          <div className="multiselect-actions">
            <button type="button" onClick={() => onChange(options.map((o) => o.path))}>
              All
            </button>
            <button type="button" onClick={() => onChange([])}>
              None
            </button>
          </div>
          {options.map((o) => (
            <label key={o.path} className="multiselect-option" role="option" aria-selected={selected.includes(o.path)}>
              <input
                type="checkbox"
                data-path={o.path}
                checked={selected.includes(o.path)}
                onChange={(e) => toggle(o.path, e.target.checked)}
              />{' '}
              {o.name}
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
