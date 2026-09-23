/**
 * Route elements (Section 9.3/9.4). Shipped so far: Home, Info (Help/Terms/
 * Privacy rendered in-app from the legacy markdown), Explore, Ordination,
 * and Visualize & Assign (see src/visualize/). Cluster, Probabilities and
 * Distances, and Euclidean Distance render structured placeholders until
 * their phases land.
 */
import { marked } from 'marked';
import type { ReactElement } from 'react';
import { useMemo } from 'react';
import helpMarkdown from '../content/help.md?raw';
import privacyMarkdown from '../content/privacy.md?raw';
import termsMarkdown from '../content/terms.md?raw';

function Placeholder({ title, phase }: { title: string; phase: string }): ReactElement {
  return (
    <section aria-labelledby={`placeholder-${phase}`}>
      <h1 id={`placeholder-${phase}`}>{title}</h1>
      <p>
        This workflow lands in {phase}. The route, navigation, and theme shell are in place
        so the parity surface can be filled without structural rework.
      </p>
    </section>
  );
}

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

export function ExplorePage(): ReactElement {
  return <Placeholder title="Explore" phase="the Explore slice" />;
}

export function OrdinationPage(): ReactElement {
  return <Placeholder title="Ordination" phase="the Ordination slice" />;
}

export function ClusterPage(): ReactElement {
  return <Placeholder title="Cluster" phase="Phase 6" />;
}

export function ProbabilitiesPage(): ReactElement {
  return <Placeholder title="Probabilities and Distances" phase="Phase 6" />;
}

export function EuclideanPage(): ReactElement {
  return <Placeholder title="Euclidean Distance" phase="Phase 6" />;
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
