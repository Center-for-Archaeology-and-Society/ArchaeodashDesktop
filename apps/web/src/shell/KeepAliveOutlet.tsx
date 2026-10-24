/**
 * Route outlet that keeps visited pages mounted: leaving a page hides it
 * instead of unmounting it, so its settings, results, and scroll position
 * are still there when you come back. Pages remount only when the project
 * or its data change (the parent's `key`).
 */
import { useRef, type ReactElement, type ReactNode } from 'react';
import { useLocation, useOutlet } from 'react-router';

export function KeepAliveOutlet(): ReactElement {
  const outlet = useOutlet();
  const { pathname } = useLocation();
  const pages = useRef(new Map<string, ReactNode>());
  if (outlet) pages.current.set(pathname, outlet);
  return (
    <>
      {[...pages.current].map(([path, element]) => (
        <div key={path} className="keep-alive-page" hidden={path !== pathname}>
          {element}
        </div>
      ))}
    </>
  );
}
