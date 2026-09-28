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
    <section aria-label={title}>
      <h1>{title}</h1>
      <div className="markdown-body" dangerouslySetInnerHTML={{ __html: html }} />
    </section>
  );
}

export function HomePage(): ReactElement {
  return (
    <section aria-labelledby="home-heading">
      <h1 id="home-heading">Home</h1>
      <p>
        ArchaeoDash — a dashboard for archaeological compositional analysis. Open or import a
        dataset from the Data Manager to begin.
      </p>
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
