/** Static content routes. Analysis routes are wired in main.tsx. */
import { marked } from 'marked';
import { createContext, useContext, useMemo, useRef, type MouseEvent, type ReactElement } from 'react';
import { Link } from 'react-router';
import helpMarkdown from '../content/help.md?raw';
import privacyMarkdown from '../content/privacy.md?raw';
import termsMarkdown from '../content/terms.md?raw';
import { ExternalLink, followExternal, isExternalUrl, openHelpWindow } from '../ui/external.tsx';
import { errorText, notify } from '../ui/toast-store.ts';

/** Where bugs and feature requests go (legacy Home tab, R/homeTab.R). */
export const ISSUES_URL = 'https://github.com/Center-for-Archaeology-and-Society/Archaeodash/issues';
/** The hosted version of ArchaeoDash (legacy Home tab). */
export const HOSTED_URL = 'https://cas.rc.asu.edu/app/Archaeodash/';

/** App facts the shell shares with static pages. */
export interface ShellInfo {
  readonly appInfo?: { readonly app: string; readonly version: string; readonly ready: boolean };
  /** True in the desktop app (local project folders, no hosted accounts). */
  readonly desktop: boolean;
}

export const ShellInfoContext = createContext<ShellInfo>({ desktop: false });

export interface HeadingEntry {
  readonly id: string;
  readonly text: string;
  readonly depth: number;
}

const ENTITIES: Record<string, string> = { '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&#39;': "'" };

/** Visible text of an inline HTML fragment. */
function plainText(html: string): string {
  return html.replace(/<[^>]*>/g, '').replace(/&(amp|lt|gt|quot|#39);/g, (entity) => ENTITIES[entity] ?? entity).trim();
}

/** GitHub-style heading slug: lowercase words joined by hyphens. */
export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .trim()
    .replace(/\s+/g, '-');
}

/**
 * Renders trusted, repo-controlled markdown (not user input) to HTML and
 * gives every heading an id (`prefix` + slug, de-duplicated) for the table
 * of contents and in-page links.
 */
export function renderMarkdown(markdown: string, prefix = ''): { html: string; headings: HeadingEntry[] } {
  const raw = marked.parse(markdown, { async: false });
  const headings: HeadingEntry[] = [];
  const used = new Map<string, number>();
  const html = raw.replace(/<h([1-6])>([\s\S]*?)<\/h\1>/g, (_match, level: string, inner: string) => {
    const text = plainText(inner);
    const base = `${prefix}${slugify(text) || 'section'}`;
    const seen = used.get(base) ?? 0;
    used.set(base, seen + 1);
    const id = seen === 0 ? base : `${base}-${seen}`;
    headings.push({ id, text, depth: Number(level) });
    return `<h${level} id="${id}" tabindex="-1">${inner}</h${level}>`;
  });
  return { html, headings };
}

/** Scrolls to and focuses the heading with `id` inside `root`. */
function goToHeading(root: HTMLElement | null, id: string): boolean {
  const target = root?.querySelector<HTMLElement>(`[id="${id.replace(/"/g, '')}"]`);
  if (!target) return false;
  target.scrollIntoView({ behavior: 'smooth', block: 'start' });
  target.focus({ preventScroll: true });
  return true;
}

export interface MarkdownPageProps {
  readonly markdown: string;
  readonly title: string;
  /** Table of contents beside the text (shown when there are 3+ sections). */
  readonly toc?: boolean;
  /** Extra controls in the page header (e.g. "Open in new window"). */
  readonly actions?: ReactElement;
}

export function MarkdownPage({ markdown, title, toc = true, actions }: MarkdownPageProps): ReactElement {
  const prefix = `${slugify(title)}-`;
  const { html, headings } = useMemo(() => renderMarkdown(markdown, prefix), [markdown, prefix]);
  const body = useRef<HTMLDivElement | null>(null);
  // h2/h3 make the contents; the document's own h1 is the page title.
  const sections = headings.filter((heading) => heading.depth === 2 || heading.depth === 3);
  const showToc = toc && sections.length >= 3;

  // In-page links scroll within this page (several pages stay mounted, so
  // the browser's own #hash jump could pick a hidden copy); https links open
  // outside the app.
  const onBodyClick = (event: MouseEvent<HTMLDivElement>) => {
    const link = (event.target as HTMLElement).closest?.('a');
    const href = link?.getAttribute('href') ?? '';
    if (href.startsWith('#')) {
      event.preventDefault();
      const hash = decodeURIComponent(href.slice(1));
      if (!goToHeading(body.current, `${prefix}${slugify(hash)}`)) goToHeading(body.current, hash);
    } else if (isExternalUrl(href)) {
      event.preventDefault();
      followExternal(href);
    }
  };

  return (
    <section className="page" aria-label={title}>
      <div className="markdown-page-header">
        <h1>{title}</h1>
        {actions}
      </div>
      <div className={showToc ? 'markdown-layout has-toc' : 'markdown-layout'}>
        {showToc && (
          <nav className="markdown-toc" aria-label={`${title} contents`}>
            <p className="markdown-toc-heading">Contents</p>
            <ol>
              {sections.map((heading) => (
                <li key={heading.id} className={heading.depth === 3 ? 'toc-sub' : undefined}>
                  <a
                    href={`#${heading.id}`}
                    onClick={(event) => {
                      event.preventDefault();
                      goToHeading(body.current, heading.id);
                    }}
                  >
                    {heading.text}
                  </a>
                </li>
              ))}
            </ol>
          </nav>
        )}
        <div ref={body} className="card markdown-body" onClick={onBodyClick} dangerouslySetInnerHTML={{ __html: html }} />
      </div>
    </section>
  );
}

/** Opens Help separately; failures become an error toast. */
function openHelpSeparately(): void {
  void openHelpWindow().catch((error: unknown) => notify({ kind: 'error', text: `Could not open Help in a new window: ${errorText(error)}` }));
}

/** "Open in new window" for Help (desktop window or browser window). */
export function OpenHelpWindowButton({ inline = false }: { inline?: boolean }): ReactElement {
  return (
    <button type="button" className={inline ? 'link-button' : 'btn'} onClick={openHelpSeparately}>
      {inline ? 'open it in a new window' : 'Open in new window'}
    </button>
  );
}

export function HomePage(): ReactElement {
  const { appInfo, desktop } = useContext(ShellInfoContext);
  return (
    <section className="page" aria-labelledby="home-heading">
      <div className="card home-hero">
        <img
          className="home-logo"
          src="/logo-256.png"
          srcSet="/logo-256.png 256w, /logo-512.png 512w"
          sizes="120px"
          width={120}
          height={120}
          alt="ArchaeoDash logo"
        />
        <div>
          <p className="eyebrow">Home</p>
          <h1 id="home-heading">Welcome to ArchaeoDash</h1>
          <p className="page-description">
            A user-friendly application for the statistical analysis and visualization of geochemical
            compositional data of archaeological materials. Use the Data Manager in the sidebar to import a
            source file (Import…) and manage the project&apos;s group files, then choose groups in an analysis view.
          </p>
          <p className="home-help-link">
            <strong>Help guide:</strong> <Link to="/info/help">Open the ArchaeoDash Help</Link>
            {' · '}
            <OpenHelpWindowButton inline />
          </p>
        </div>
      </div>
      <ol className="home-steps" aria-label="Getting started">
        <li><strong>Import data</strong>Bring in a CSV, TSV, or Excel file; each group becomes its own file.</li>
        <li><strong>Set up the analysis</strong>Choose elements, imputation, and a transform once in the Data Manager.</li>
        <li><strong>Explore</strong>Check missing values, distributions, and the samples themselves.</li>
        <li><strong>Analyze and assign</strong>Ordinate, cluster, and move samples into groups.</li>
      </ol>
      <section className="card home-about" aria-labelledby="home-data-heading">
        <h2 id="home-data-heading">Your data</h2>
        {desktop ? (
          <p>
            Your data stays on your computer, in the project folder you open. ArchaeoDash does not upload it or
            share it with anyone, and you keep all rights to it. Back up your project folders as you would any
            other research files.
          </p>
        ) : (
          <p>
            Users may create an account where data will be stored on a server. Users retain all rights to their
            data and may delete it at any time. Data will not be shared with any third party. Users are welcome to
            use the app without creating an account, but no guarantees are made regarding the persistence of data
            with or without an account.
          </p>
        )}
      </section>
      <section className="card home-about" aria-labelledby="home-about-heading">
        <h2 id="home-about-heading">About</h2>
        <p>
          ArchaeoDash is in beta testing: it is functional and you are welcome to use it for your data. See the
          Info menu for help, terms and conditions, and the privacy policy.
        </p>
        <p>
          No warranties are made and the app is provided as is under the MIT license. ArchaeoDash is open source
          and contributions to the code base are welcome.
        </p>
        <p>
          Please submit bugs and feature requests on the{' '}
          <ExternalLink href={ISSUES_URL}>GitHub issues page</ExternalLink>.
        </p>
        <p>
          Supported by the Center for Archaeology and Society at Arizona State University.
          {desktop && (
            <>
              {' '}An online version is hosted at <ExternalLink href={HOSTED_URL}>cas.rc.asu.edu/app/Archaeodash/</ExternalLink>.
            </>
          )}
        </p>
        <p className="home-version" aria-label="Version">
          {appInfo ? `${appInfo.app} version ${appInfo.version}` : 'Version information unavailable'}
        </p>
      </section>
    </section>
  );
}

export function HelpPage({ standalone = false }: { standalone?: boolean }): ReactElement {
  return <MarkdownPage markdown={helpMarkdown} title="Help" actions={standalone ? undefined : <OpenHelpWindowButton />} />;
}

export function TermsPage(): ReactElement {
  return <MarkdownPage markdown={termsMarkdown} title="Terms & Conditions" />;
}

export function PrivacyPage(): ReactElement {
  return <MarkdownPage markdown={privacyMarkdown} title="Privacy Policy" />;
}

/**
 * The separate Help window (desktop window or browser popup): help only, no
 * shell and no backend calls. Follows theme changes made in the main window.
 */
export function HelpWindow(): ReactElement {
  return (
    <main className="help-window main-panel">
      <HelpPage standalone />
    </main>
  );
}
