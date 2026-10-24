/**
 * Shell features: Home/About, help contents, toasts, job activity, the
 * per-page error boundary, the Info menu, and desktop-local theme storage.
 * node:test + renderToString (no DOM).
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import { MemoryRouter } from 'react-router';
import type { ReactElement } from 'react';
import { HelpPage, HomePage, HOSTED_URL, ISSUES_URL, renderMarkdown, ShellInfoContext, slugify, type ShellInfo } from './routes.tsx';
import { AppShell, NavMenu } from './AppShell.tsx';
import { navRoutes } from './nav.ts';
import { ToastStore } from '../ui/toast-store.ts';
import { Toasts } from '../ui/Toasts.tsx';
import { PageErrorBoundary } from '../ui/ErrorBoundary.tsx';
import { ActivityTracker, JobActivity, jobFinishedNotice, returnsJobSnapshot } from '../activity.ts';
import { hydrateTheme, saveThemePreference, themeSyncsWithBackend } from '../theme.ts';
import type { Transport } from '@archaeodash/client';

function home(info: ShellInfo): string {
  return renderToString(
    <MemoryRouter>
      <ShellInfoContext.Provider value={info}>
        <HomePage />
      </ShellInfoContext.Provider>
    </MemoryRouter>,
  );
}

test('home shows the legacy welcome, help link, About, and the app version', () => {
  const html = home({ desktop: true, appInfo: { app: 'ArchaeoDash', version: '1.2.3', ready: true } });
  assert.ok(html.includes('Welcome to ArchaeoDash'));
  assert.ok(html.includes('href="/info/help"'), 'help link');
  assert.ok(html.includes('open it in a new window'), 'open help separately');
  assert.ok(html.includes('beta testing'), 'status wording');
  assert.ok(html.includes('MIT license') && html.includes('No warranties'), 'license notice');
  assert.ok(html.includes(`href="${ISSUES_URL}"`), 'GitHub issues link');
  assert.ok(html.includes('Center for Archaeology and Society'), 'support attribution');
  assert.ok(html.includes('ArchaeoDash version 1.2.3'), 'version from appInfo');
  assert.ok(html.includes('stays on your computer'), 'desktop data guidance');
  assert.ok(html.includes(`href="${HOSTED_URL}"`), 'desktop points to the online version');
  assert.ok(html.includes('rel="noopener noreferrer"'));
});

test('hosted home keeps the account data wording', () => {
  const html = home({ desktop: false });
  assert.ok(html.includes('Users retain all rights'));
  assert.ok(!html.includes('stays on your computer'));
  assert.ok(html.includes('Version information unavailable'));
});

test('markdown headings get unique, prefixed ids', () => {
  const { html, headings } = renderMarkdown('# Top\n\n## Data & Files\n\ntext\n\n## Data & Files\n\n### Sub *one*', 'help-');
  assert.deepEqual(headings.map((h) => h.id), ['help-top', 'help-data-files', 'help-data-files-1', 'help-sub-one']);
  assert.equal(headings[1]!.text, 'Data & Files');
  assert.ok(html.includes('<h2 id="help-data-files" tabindex="-1">'));
  assert.equal(slugify('VISUALIZE & ASSIGN'), 'visualize-assign');
});

test('help has a table of contents and an open-separately action', () => {
  const html = renderToString(<HelpPage />);
  assert.ok(html.includes('aria-label="Help contents"'), 'contents nav');
  assert.match(html, /<nav[^>]*Help contents[\s\S]*href="#help-[a-z0-9-]+"/, 'contents link to a section');
  assert.match(html, /<h2 id="help-[a-z0-9-]+" tabindex="-1">/, 'section anchor');
  assert.ok(html.includes('Open in new window'));
  const standalone = renderToString(<HelpPage standalone />);
  assert.ok(!standalone.includes('Open in new window'), 'no button inside the help window itself');
});

test('toast store adds, caps, and dismisses notifications', () => {
  const store = new ToastStore();
  const first = store.notify({ text: 'one', durationMs: 0 });
  for (let i = 0; i < 5; i += 1) store.notify({ kind: 'success', text: `n${i}`, durationMs: 0 });
  assert.equal(store.snapshot().length, 5, 'at most five at once');
  assert.ok(!store.snapshot().some((t) => t.id === first), 'oldest dropped');
  const last = store.snapshot().at(-1)!;
  store.dismiss(last.id);
  assert.equal(store.snapshot().length, 4);
  store.notify({ kind: 'error', text: 'Broken', durationMs: 0 });
  const html = renderToString(<Toasts store={store} />);
  assert.ok(html.includes('aria-live="polite"'));
  assert.ok(html.includes('role="alert"') && html.includes('Broken'), 'errors are alerts');
  assert.ok(html.includes('aria-label="Dismiss notification"'));
});

test('the shell mounts the toast region', () => {
  const html = renderToString(
    <MemoryRouter initialEntries={['/']}>
      <AppShell theme="light" onThemeChange={() => {}} />
    </MemoryRouter>,
  );
  assert.ok(html.includes('class="toasts"'));
});

test('tracker.begin counts until ended, and ending twice is harmless', () => {
  const tracker = new ActivityTracker();
  const end = tracker.begin('Running analysis');
  assert.deepEqual(tracker.snapshot(), { count: 1, label: 'Running analysis' });
  end();
  end();
  assert.deepEqual(tracker.snapshot(), { count: 0, label: '' });
});

test('analysis jobs count as activity until a snapshot shows them finished', () => {
  const tracker = new ActivityTracker();
  const finished: string[] = [];
  const jobs = new JobActivity(tracker, (job) => finished.push(`${job.id}:${job.state}`), 60_000);
  jobs.observe({ id: 'a', state: 'queued' });
  jobs.observe({ id: 'a', state: 'running' });
  jobs.observe({ id: 'b', state: 'running' });
  jobs.observe({ unrelated: true });
  assert.equal(tracker.snapshot().count, 2);
  jobs.observe({ id: 'a', state: 'succeeded' });
  jobs.observe({ id: 'a', state: 'succeeded' });
  assert.equal(tracker.snapshot().count, 1);
  jobs.observe({ id: 'b', state: 'failed', error: { message: 'boom' } });
  assert.equal(tracker.snapshot().count, 0);
  assert.deepEqual(finished, ['a:succeeded', 'b:failed'], 'one notice per job');
  // A job that was never seen running (e.g. finished at submit) is ignored.
  jobs.observe({ id: 'c', state: 'succeeded' });
  assert.equal(finished.length, 2);
});

test('job notices: failures always, success only for long jobs, cancel silent', () => {
  assert.equal(jobFinishedNotice({ id: 'a', state: 'succeeded' }, 500), null);
  assert.deepEqual(jobFinishedNotice({ id: 'a', state: 'succeeded' }, 5000), { kind: 'success', text: 'Analysis finished.' });
  assert.deepEqual(jobFinishedNotice({ id: 'a', state: 'failed', error: { message: 'boom' } }, 10), { kind: 'error', text: 'Analysis failed: boom' });
  assert.equal(jobFinishedNotice({ id: 'a', state: 'cancelled' }, 9000), null);
  for (const key of ['submit_analysis_job', 'get_analysis_job', 'cancel_analysis_job', 'POST /api/v1/jobs', 'GET /api/v1/jobs/j1', 'POST /api/v1/jobs/j1/cancel']) {
    assert.ok(returnsJobSnapshot(key), key);
  }
  assert.ok(!returnsJobSnapshot('watch_analysis_job'));
  assert.ok(!returnsJobSnapshot('GET /api/v1/jobs/j1/events'));
});

test('page error boundary shows the error with Try again instead of the page', () => {
  assert.ok(PageErrorBoundary.getDerivedStateFromError(new Error('x')).error instanceof Error);
  const boundary = new PageErrorBoundary({ children: null, label: 'Explore' });
  boundary.state = { error: new Error('chart exploded'), attempt: 0 };
  const html = renderToString(boundary.render() as ReactElement);
  assert.ok(html.includes('Something went wrong on Explore'));
  assert.ok(html.includes('chart exploded'));
  assert.ok(html.includes('Try again'));
  assert.ok(html.includes('role="alert"'));
});

test('info menu offers Show log folder on desktop and opens by click only', () => {
  const info = navRoutes.find((route) => route.label === 'Info')!;
  const closed = renderToString(<MemoryRouter><ul><NavMenu route={info} onShowLogFolder={() => {}} /></ul></MemoryRouter>);
  assert.ok(closed.includes('aria-expanded="false"'));
  assert.ok(!closed.includes('Show log folder'));
  const open = renderToString(<MemoryRouter><ul><NavMenu route={info} onShowLogFolder={() => {}} initiallyOpen /></ul></MemoryRouter>);
  assert.ok(open.includes('Show log folder'));
  assert.ok(open.includes('href="/info/help"'));
  const web = renderToString(<MemoryRouter><ul><NavMenu route={info} initiallyOpen /></ul></MemoryRouter>);
  assert.ok(!web.includes('Show log folder'));
});

test('desktop keeps the theme locally; hosted syncs it with the server', async () => {
  let gets = 0;
  const puts: string[] = [];
  const preferences = {
    get: async () => { gets += 1; return { preferences: [{ key: 'theme', value: 'dark' }] }; },
    put: async (_key: string, value: string) => { puts.push(value); },
  } as unknown as Transport['preferences'];
  const desktop = { kind: 'tauri' as const, preferences };
  const hosted = { kind: 'http' as const, preferences };
  assert.equal(themeSyncsWithBackend(desktop), false);
  assert.equal(themeSyncsWithBackend(hosted), true);
  assert.equal(await hydrateTheme(desktop, 'light'), 'light', 'stale project preference ignored');
  assert.equal(gets, 0, 'desktop never reads the project preference');
  await saveThemePreference(desktop, 'dark');
  assert.deepEqual(puts, [], 'desktop never writes the project preference');
  await saveThemePreference(hosted, 'dark');
  assert.deepEqual(puts, ['dark']);
});
