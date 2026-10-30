/**
 * Hosted mode: analysis gating, the shell's hosted controls, the session
 * observer, and the remembered last project. node:test + renderToString.
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import { MemoryRouter } from 'react-router';
import type { AuthService, SessionInfo } from '@archaeodash/client';
import { AppShell } from './AppShell.tsx';
import { GuestContext, GuestStart, HostedGate, HostedWorkspaceContext, isAnalysisPath, observeSession, readLastProject, saveLastProject } from './hosted.tsx';
import { HostedProjectsPage } from './HostedProjectsPage.tsx';

test('analysis pages need an open hosted project; home, account, projects and info do not', () => {
  for (const path of ['/explore', '/visualize/', '/ordination', '/cluster', '/probabilities', '/euclidean', '/data']) {
    assert.ok(isAnalysisPath(path), path);
  }
  for (const path of ['/', '/account', '/projects', '/info/help', '/auth/verify', '/explorer']) {
    assert.ok(!isAnalysisPath(path), path);
  }
});

test('the gate asks to sign in, then to open a project', () => {
  const signedOut = renderToString(<MemoryRouter><HostedGate state="signed-out" /></MemoryRouter>);
  assert.ok(signedOut.includes('Sign in to continue'));
  assert.ok(signedOut.includes('href="/account"'));
  const noProject = renderToString(<MemoryRouter><HostedGate state="no-project" /></MemoryRouter>);
  assert.ok(noProject.includes('Open a project'));
  assert.ok(noProject.includes('href="/projects"'));
});

function shell(path: string, hosted: { username: string | null; gate?: ReturnType<typeof HostedGate> }, projectName = '', projectPath = ''): string {
  return renderToString(
    <MemoryRouter initialEntries={[path]}>
      <AppShell theme="light" onThemeChange={() => {}} hosted={hosted} projectName={projectName} projectPath={projectPath} />
    </MemoryRouter>,
  );
}

test('hosted shell shows the gate on analysis pages only, with the user and project in the top bar', () => {
  const gate = <HostedGate state="no-project" />;
  const explore = shell('/explore', { username: 'ada', gate });
  assert.ok(explore.includes('Choose a project'), 'gate replaces the analysis page');
  assert.ok(explore.includes('>ada<'), 'signed-in user');
  assert.ok(explore.includes('Open Project'));
  assert.ok(explore.includes('Sign in and open a project'), 'sidebar note');
  const account = shell('/account', { username: 'ada', gate });
  assert.ok(!account.includes('Choose a project'), 'account page is never gated');
  const open = shell('/explore', { username: 'ada' }, 'Site A', '0197aaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa');
  assert.ok(open.includes('Site A') && open.includes('Switch Project'));
  assert.ok(!open.includes('Choose a project'));
  const signedOut = shell('/explore', { username: null, gate: <HostedGate state="signed-out" /> });
  assert.ok(signedOut.includes('>Sign in<'));
  assert.ok(!signedOut.includes('Open Project'), 'no project switch while signed out');
});

test('observeSession reports successful sign-in and sign-out, not failures', async () => {
  let changes = 0;
  let fail = false;
  const session: SessionInfo = { authenticated: true, username: 'ada' };
  const auth = {
    login: async () => {
      if (fail) throw new Error('invalid_credentials');
      return session;
    },
    logout: async () => {},
    logoutAll: async () => {},
    session: async () => session,
  } as unknown as AuthService;
  const observed = observeSession(auth, () => {
    changes += 1;
  });
  assert.deepEqual(await observed.login({ identifier: 'ada', password: 'x' }), session);
  await observed.logout();
  await observed.logoutAll();
  assert.equal(changes, 3);
  fail = true;
  await assert.rejects(observed.login({ identifier: 'ada', password: 'x' }));
  assert.equal(changes, 3);
  assert.equal(await observed.session(), session, 'other calls pass through');
});

test('guest start offers an account-free session, then explains it while open', () => {
  const render = (value: { active: boolean; starting: boolean } | null) =>
    renderToString(
      <MemoryRouter>
        <GuestContext.Provider value={value && { ...value, start: () => {} }}>
          <HostedGate state="signed-out" />
          <GuestStart />
        </GuestContext.Provider>
      </MemoryRouter>,
    );
  assert.ok(!render(null).includes('Continue without an account'), 'hidden when the server has no guests');
  const offered = render({ active: false, starting: false });
  assert.ok(offered.includes('Continue without an account'));
  assert.ok(offered.includes('Nothing is kept after you close or reload the page'));
  assert.ok(render({ active: false, starting: true }).includes('Starting…'));
  const open = render({ active: true, starting: false });
  assert.ok(open.includes('You are using a guest session'));
  assert.ok(!open.includes('Continue without an account'));
});

test('the top bar marks a guest session as temporary', () => {
  const html = shell('/explore', { username: null, guest: true } as never, 'Guest session', 'guest-1');
  assert.ok(html.includes('Guest session'));
  assert.ok(html.includes('Temporary'));
  assert.ok(html.includes('Sign in'));
});

test('the last project is remembered per user and survives missing storage', () => {
  const store = new Map<string, string>();
  const g = globalThis as { window?: unknown };
  const before = g.window;
  g.window = {
    localStorage: {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    },
  };
  try {
    saveLastProject('ada', 'p1');
    saveLastProject('bob', 'p2');
    assert.equal(readLastProject('ada'), 'p1');
    assert.equal(readLastProject('bob'), 'p2');
    saveLastProject('ada', null);
    assert.equal(readLastProject('ada'), null);
  } finally {
    g.window = before;
  }
  // No window (or storage denied): nothing remembered, nothing thrown.
  assert.equal(readLastProject('ada'), null);
  saveLastProject('ada', 'p1');
});

test('the projects page lists, creates and opens projects; importing is left to the Data Manager', () => {
  const projects = { list: async () => ({ projects: [] }) } as never;
  const plain = renderToString(<HostedProjectsPage projects={projects} />);
  assert.ok(plain.includes('Create project'));
  const hosted = renderToString(
    <HostedWorkspaceContext.Provider value={{ projectId: '', open: () => {} }}>
      <HostedProjectsPage projects={projects} />
    </HostedWorkspaceContext.Provider>,
  );
  assert.ok(hosted.includes('Create project'));
  assert.ok(hosted.includes('Import source files from the Data Manager'));
  // The old catalog upload panel and its quota line are gone: those files
  // never reached the project the analysis pages use.
  assert.ok(!hosted.includes('Upload'));
  assert.ok(!hosted.includes('bytes used'));
});

test('storage usage counts catalog and workspace bytes', async () => {
  const { usageText } = await import('./HostedProjectsPage.tsx');
  assert.equal(usageText(null), null);
  assert.equal(
    usageText({ logical_bytes: 500, reserved_bytes: 0, file_count: 1, workspace_bytes: 12_000_000, limit_bytes: 1_073_741_824 }),
    '12.0 MB of 1.1 GB used',
  );
  assert.equal(usageText({ logical_bytes: 0, reserved_bytes: 0, file_count: 0, limit_bytes: 1000 }), '0 bytes of 1.0 KB used');
});
