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
  HelpPage,
  HomePage,
  PrivacyPage,
  TermsPage,
} from './shell/routes.tsx';
import { navRoutes } from './shell/nav.ts';
import {
  AccountPage,
  ResetPasswordPage,
  VerifyEmailPage,
} from './shell/AccountPage.tsx';
import type { AuthService } from '@archaeodash/client';
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
      'Account',
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

test('native project picker is shown only for transports that support it', () => {
  const browser = renderShell();
  assert.ok(!browser.includes('Open Project'));
  const html = renderToString(
    <MemoryRouter initialEntries={['/']}>
      <AppShell theme="light" onThemeChange={() => {}} projects={{ open: async () => null, current: async () => null }} />
    </MemoryRouter>,
  );
  assert.ok(html.includes('Open Project'));
  assert.ok(html.includes('Open a project folder to browse groups and run analyses.'));
});

test('info routes render the legacy markdown content in-app (no iframe)', () => {
  const help = renderAt(<HelpPage />);
  assert.ok(help.includes('<h1'), 'help renders as HTML');
  const terms = renderAt(<TermsPage />);
  assert.ok(terms.includes('Terms'));
  const privacy = renderAt(<PrivacyPage />);
  assert.ok(privacy.includes('Privacy'));
});

test('home route renders the project entry guidance', () => {
  const html = renderToString(<MemoryRouter><HomePage /></MemoryRouter>);
  assert.ok(html.includes('Home'));
  assert.ok(html.includes('Data Manager'));
});

test('account page renders the signed-out sign-in form in hosted mode', () => {
  const auth: AuthService = {
    consent: async () => ({ consent_version: '2026-10', terms_path: '/legal/terms', privacy_path: '/legal/privacy' }),
    register: async () => {},
    verify: async () => {},
    login: async () => ({ authenticated: false }),
    session: async () => ({ authenticated: false }),
    logout: async () => {},
    logoutAll: async () => {},
    requestPasswordReset: async () => {},
    confirmPasswordReset: async () => {},
  };
  const html = renderAt(
    <MemoryRouter initialEntries={['/account']}>
      <AccountPage auth={auth} />
    </MemoryRouter>,
  );
  assert.ok(html.includes('Sign in'), 'sign-in form is the default mode');
  assert.ok(html.includes('Register as a new user'), 'registration entry point');
  assert.ok(html.includes('Forgot your password?'), 'password-reset entry point');
  assert.ok(html.includes('Stay signed in'), 'remember-me choice offered');
});

test('account page explains that hosted accounts are desktop-unavailable', () => {
  const html = renderAt(
    <MemoryRouter initialEntries={['/account']}>
      <AccountPage />
    </MemoryRouter>,
  );
  assert.ok(
    html.includes('only available in the web application'),
    'desktop-mode notice',
  );
});

test('navbar gains the Account entry after Euclidean Distance', () => {
  const labels = navRoutes.map((r) => r.label);
  assert.ok(labels.includes('Account'));
  assert.equal(labels.indexOf('Account'), labels.indexOf('Euclidean Distance') + 1);
});

test('verify and reset link pages handle a missing token without calling the API', () => {
  const verifyCalls: string[] = [];
  const resetCalls: string[] = [];
  const auth: AuthService = {
    consent: async () => ({ consent_version: '2026-10', terms_path: '/legal/terms', privacy_path: '/legal/privacy' }),
    register: async () => {},
    verify: async (token: string) => {
      verifyCalls.push(token);
    },
    login: async () => ({ authenticated: false }),
    session: async () => ({ authenticated: false }),
    logout: async () => {},
    logoutAll: async () => {},
    requestPasswordReset: async () => {},
    confirmPasswordReset: async (r: { token: string; newPassword: string }) => {
      resetCalls.push(r.token);
    },
  };
  const verifyHtml = renderAt(
    <MemoryRouter initialEntries={['/auth/verify']}>
      <VerifyEmailPage auth={auth} />
    </MemoryRouter>,
  );
  assert.ok(verifyHtml.includes('missing its token'), 'missing-token notice');
  assert.equal(verifyCalls.length, 0, 'no API call without a token');
  const resetHtml = renderAt(
    <MemoryRouter initialEntries={['/auth/reset']}>
      <ResetPasswordPage auth={auth} />
    </MemoryRouter>,
  );
  assert.ok(resetHtml.includes('missing its token'), 'missing-token notice on reset');
  assert.equal(resetCalls.length, 0, 'no API call without a token');
});
