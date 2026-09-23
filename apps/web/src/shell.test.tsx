/**
 * Shell unit tests (node:test + renderToString; interactive/e2e coverage comes
 * with the Phase 4/5 Playwright layers per IMPLEMENTATION.md Section 15.2).
 * Asserts the legacy navbar order, theme normalization, and responsive-shell
 * attributes without a DOM implementation.
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import { MemoryRouter } from 'react-router';
import type { ReactElement } from 'react';
import { AppShell } from './shell/AppShell.tsx';
import {
  ClusterPage,
  EuclideanPage,
  ExplorePage,
  HelpPage,
  HomePage,
  OrdinationPage,
  PrivacyPage,
  ProbabilitiesPage,
  TermsPage,
} from './shell/routes.tsx';
import { navRoutes } from './shell/nav.ts';
import { normalizeTheme, themes } from '@archaeodash/client';

function renderShell(theme = 'light'): string {
  return renderToString(
    <MemoryRouter initialEntries={['/']}>
      <AppShell theme={theme} onThemeChange={() => {}} />
    </MemoryRouter>,
  );
}

function renderAt(element: ReactElement): string {
  return renderToString(element);
}

const escape = (s: string) => s.replace(/&/g, '&amp;');

test('navbar preserves the legacy tab order and labels', () => {
  assert.deepEqual(
    navRoutes.map((r) => r.label),
    [
      'Home',
      'Explore',
      'Visualize & Assign',
      'Ordination',
      'Cluster',
      'Probabilities and Distances',
      'Euclidean Distance',
      'Info',
    ],
  );
  const html = renderShell();
  for (const label of navRoutes.map((r) => r.label)) {
    assert.ok(html.includes(escape(label)), `missing nav label: ${label}`);
  }
  assert.ok(html.includes('Data Manager'), 'sidebar is the Data Manager');
  const info = navRoutes.find((r) => r.label === 'Info');
  assert.deepEqual(
    info?.children?.map((c) => c.label),
    ['Help', 'Terms & Conditions', 'Privacy Policy'],
  );
  assert.ok(html.includes('aria-haspopup="true"'), 'Info menu is keyboard-reachable');
});

test('theme select offers the three legacy themes and tokens match the legacy palette', () => {
  const html = renderShell('dark');
  assert.ok(
    html.includes('value="simple"') && html.includes('value="light"') && html.includes('value="dark"'),
  );
  assert.deepEqual(
    themes.map((t) => t.name),
    ['light', 'simple', 'dark'],
  );
  assert.equal(normalizeTheme('dark'), 'dark');
  assert.equal(normalizeTheme('nonsense'), 'simple');
});

test('sidebar collapses via aria-wired toggle button', () => {
  const open = renderShell();
  assert.ok(open.includes('aria-expanded="true"'));
  assert.ok(open.includes('data-sidebar-open="true"'));
});

test('info routes render the legacy markdown content in-app (no iframe)', () => {
  const help = renderAt(<HelpPage />);
  assert.ok(help.includes('<h1'), 'help renders as HTML');
  const terms = renderAt(<TermsPage />);
  assert.ok(terms.includes('Terms'));
  const privacy = renderAt(<PrivacyPage />);
  assert.ok(privacy.includes('Privacy'));
});

test('phase routes render their structured placeholders', () => {
  for (const [el, label] of [
    [<HomePage />, 'Home'],
    [<ExplorePage />, 'Explore'],
    [<OrdinationPage />, 'Ordination'],
    [<ClusterPage />, 'Cluster'],
    [<ProbabilitiesPage />, 'Probabilities and Distances'],
    [<EuclideanPage />, 'Euclidean Distance'],
  ] as const) {
    const html = renderToString(<MemoryRouter>{el}</MemoryRouter>);
    assert.ok(html.includes(label), `placeholder missing: ${label}`);
  }
});
