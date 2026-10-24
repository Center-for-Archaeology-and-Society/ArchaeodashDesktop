/**
 * Tab panels that stay mounted once opened: switching tabs hides a panel
 * instead of unmounting it, so its results, settings, and scroll position
 * are still there when you come back (nothing is recomputed). Panels that
 * were never opened are not rendered.
 */
import { useState, type ReactElement, type ReactNode } from 'react';

export function TabPanels<K extends string>({
  active,
  panels,
}: {
  active: K;
  /** Render functions, called only once a tab has been opened. */
  panels: Readonly<Record<K, () => ReactNode>>;
}): ReactElement {
  const [visited, setVisited] = useState<ReadonlySet<K>>(() => new Set([active]));
  if (!visited.has(active)) setVisited(new Set([...visited, active]));
  return (
    <>
      {(Object.keys(panels) as K[])
        .filter((key) => key === active || visited.has(key))
        .map((key) => (
          <div key={key} role="tabpanel" hidden={key !== active} className="tab-panel">
            {panels[key]()}
          </div>
        ))}
    </>
  );
}
