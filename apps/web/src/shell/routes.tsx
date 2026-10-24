/** Static content routes. Analysis routes are wired in main.tsx. */
import { marked } from 'marked';
import type { ReactElement } from 'react';
import { useMemo } from 'react';
import helpMarkdown from '../content/help.md?raw';
import privacyMarkdown from '../content/privacy.md?raw';
import termsMarkdown from '../content/terms.md?raw';

/** Renders trusted, repo-controlled markdown (not user input) to HTML. */
export function MarkdownPage({ markdown, title }: { markdown: string; title: string }): ReactElement {
  const html = useMemo(() => marked.parse(markdown, { async: false }), [markdown]);
  return (
    <section className="page" aria-label={title}>
      <h1>{title}</h1>
      <div className="card markdown-body" dangerouslySetInnerHTML={{ __html: html }} />
    </section>
  );
}

export function HomePage(): ReactElement {
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
          <h1 id="home-heading">ArchaeoDash</h1>
          <p className="page-description">
            A dashboard for archaeological compositional analysis. Use the Data Manager in the sidebar to
            import a source file (Import…) and manage the project&apos;s group files, then choose groups in an
            analysis view.
          </p>
        </div>
      </div>
      <ol className="home-steps" aria-label="Getting started">
        <li><strong>Import data</strong>Bring in a CSV, TSV, or Excel file; each group becomes its own file.</li>
        <li><strong>Set up the analysis</strong>Choose elements, imputation, and a transform once in the Data Manager.</li>
        <li><strong>Explore</strong>Check missing values, distributions, and the samples themselves.</li>
        <li><strong>Analyze and assign</strong>Ordinate, cluster, and move samples into groups.</li>
      </ol>
    </section>
  );
}

export function HelpPage(): ReactElement {
  return <MarkdownPage markdown={helpMarkdown} title="Help" />;
}

export function TermsPage(): ReactElement {
  return <MarkdownPage markdown={termsMarkdown} title="Terms & Conditions" />;
}

export function PrivacyPage(): ReactElement {
  return <MarkdownPage markdown={privacyMarkdown} title="Privacy Policy" />;
}
