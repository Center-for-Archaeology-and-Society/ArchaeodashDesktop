/** Page title, one-line description, and the page's top-level controls. */
import type { ReactElement, ReactNode } from 'react';

export function PageHeader({
  title,
  description,
  id,
  children,
}: {
  title: string;
  description?: ReactNode;
  /** Heading id for `aria-labelledby`. */
  id?: string;
  /** Controls shown at the right (e.g. the Dataset picker). */
  children?: ReactNode;
}): ReactElement {
  return (
    <header className="page-header">
      <div className="page-heading">
        <h1 id={id}>{title}</h1>
        {description && <p className="page-description">{description}</p>}
      </div>
      {children && <div className="page-header-controls">{children}</div>}
    </header>
  );
}
