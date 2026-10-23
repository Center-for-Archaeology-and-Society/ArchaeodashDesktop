import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { MemoryRouter } from 'react-router';
import type { RecentProject } from '@archaeodash/client';
import { ProjectMenu, RecentProjectList, displayPath, openedAgo } from './RecentProjects.tsx';
import { AppShell } from './AppShell.tsx';

const recent: RecentProject[] = [
  { path: '\\\\?\\C:\\Users\\me\\Site A', name: 'Site A', last_opened_unix_secs: 1_000_000, available: true },
  { path: '/data/old-survey', name: 'old-survey', last_opened_unix_secs: 900_000, available: false },
];

test('project paths drop the Windows long-path prefix for display', () => {
  assert.equal(displayPath('\\\\?\\C:\\Users\\me\\Site A'), 'C:\\Users\\me\\Site A');
  assert.equal(displayPath('\\\\?\\UNC\\server\\share\\proj'), '\\\\server\\share\\proj');
  assert.equal(displayPath('/home/me/proj'), '/home/me/proj');
});

test('opened times read naturally', () => {
  const now = 2_000_000 * 1000;
  assert.equal(openedAgo(2_000_000 - 10, now), 'just now');
  assert.equal(openedAgo(2_000_000 - 60, now), '1 minute ago');
  assert.equal(openedAgo(2_000_000 - 3 * 3600, now), '3 hours ago');
  assert.equal(openedAgo(2_000_000 - 30 * 3600, now), 'yesterday');
  assert.equal(openedAgo(2_000_000 - 5 * 86400, now), '5 days ago');
});

test('start screen list opens available projects and flags missing folders', () => {
  const html = renderToStaticMarkup(
    <RecentProjectList recent={recent} onOpen={() => {}} onRemove={() => {}} nowMs={1_000_000 * 1000 + 120_000} />,
  );
  assert.match(html, /aria-label="Recent projects"/);
  assert.match(html, /Site A/);
  assert.match(html, /C:\\Users\\me\\Site A/);
  assert.doesNotMatch(html, /\\\\\?\\/, 'no long-path prefix shown');
  assert.match(html, /Opened 2 minutes ago/);
  assert.match(html, /Folder not found/);
  assert.match(html, /class="recent-item is-missing"><button type="button" class="recent-open" disabled=""/);
  assert.match(html, /aria-label="Remove old-survey from recent projects"/);
});

test('project menu starts closed with an accessible toggle', () => {
  const html = renderToStaticMarkup(<ProjectMenu recent={recent} onOpenRecent={() => {}} onBrowse={() => {}} />);
  assert.match(html, /aria-haspopup="menu"/);
  assert.match(html, /aria-expanded="false"/);
  assert.doesNotMatch(html, /role="menu"/);
});

test('without a project the shell shows the start screen', () => {
  const projects = { open: async () => null, current: async () => null, recent: async () => [], openRecent: async () => null };
  const html = renderToStaticMarkup(<MemoryRouter><AppShell theme="light" onThemeChange={() => {}} projects={projects} /></MemoryRouter>);
  assert.match(html, /<h1 id="start-heading">Open a project<\/h1>/);
  assert.match(html, /Open a folder…/);
  assert.match(html, /aria-label="Recent projects"/, 'menu toggle beside the project button');
  const reopening = renderToStaticMarkup(<MemoryRouter><AppShell theme="light" onThemeChange={() => {}} projects={projects} projectStatus="Reopening Site A…" /></MemoryRouter>);
  assert.match(reopening, /role="status"[^>]*><span class="spinner" aria-hidden="true"><\/span>Reopening Site A…/);
});
