// Browser-level auth lifecycle and security e2e (Phase 7 exit gate:
// "auth lifecycle/security e2e"). Drives the real hosted API through the
// real web UI in headless Chromium:
//
//   1. Security headers on API responses (CSP, nosniff, frame options).
//   2. Cookie flags: session cookie is HttpOnly (invisible to
//      document.cookie); the CSRF cookie is readable and echoed on writes.
//   3. Register (consent version) → dev-sink email link → verify page →
//      replay rejection → sign in through the UI → session view.
//   4. CSRF enforcement: a state-changing fetch without the header is 403.
//   5. Password reset round trip through the email link, old password dead.
//
// Prerequisites: PostgreSQL reachable at DATABASE_URL, the `hosted` binary
// built (cargo build -p archaeodash-api --bin hosted), Playwright chromium
// installed (PLAYWRIGHT_MODULE may point at its index.mjs), and `vite dev`
// already serving apps/web on WEB_BASE_URL (default http://127.0.0.1:4173)
// with /api proxied to the API (vite.config.ts does this for 8787).
//
// Usage: DATABASE_URL=postgres://… AUTH_PEPPER=$(64 hex) node scripts/e2e/auth-security.mjs
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const webBase = process.env.WEB_BASE_URL ?? 'http://127.0.0.1:4173';
const apiPort = process.env.API_PORT ?? '8787';
const apiBase = `http://127.0.0.1:${apiPort}`;
const tag = `e2e-${Date.now()}`;
const email = `${tag}@example.com`;
const password = 'correct horse battery staple';
const newPassword = 'even better battery staple horse';

// --- Start the hosted API with the gated dev email sink -------------------
if (!process.env.DATABASE_URL) {
  console.error('DATABASE_URL is required (the e2e drives a real Postgres).');
  process.exit(1);
}
if (!process.env.AUTH_PEPPER) {
  console.error('AUTH_PEPPER is required (64 hex characters).');
  process.exit(1);
}
const workdir = join(tmpdir(), `auth-e2e-${tag}`);
mkdirSync(workdir, { recursive: true });
const child = spawn(
  'target/debug/hosted',
  [],
  {
    cwd: join(import.meta.dirname, '..', '..'),
    env: {
      ...process.env,
      AUTH_BIND: `127.0.0.1:${apiPort}`,
      AUTH_BASE_URL: webBase,
      AUTH_EMAIL_MODE: 'dev',
      AUTH_ALLOW_DEV_EMAIL: '1',
      AUTH_LOG_FORMAT: 'json',
      AUTH_APPLY_MIGRATIONS: '1',
    },
    // The dev email sink logs action links through tracing (stdout by
    // default); a file beats a pipe for polling. Both streams go to the log.
    stdio: [
      'ignore',
      openSync(join(workdir, 'hosted.log'), 'a'),
      openSync(join(workdir, 'hosted.log'), 'a'),
    ],
  },
);
const emails = [];
const logPath = join(workdir, 'hosted.log');
const pollEmails = () => {
  try {
    const text = readFileSync(logPath, 'utf8');
    for (const line of text.split('\n')) {
      try {
        const record = JSON.parse(line);
        if (record.fields?.body) emails.push(record.fields);
      } catch {
        // partial or non-JSON lines are skipped
      }
    }
  } catch {
    // log file not created yet
  }
};
child.on('exit', (code) => {
  if (!failed) console.error(`hosted process exited early (code ${code})`);
});

const waitForApi = async () => {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const res = await fetch(`${apiBase}/api/v1/health/ready`);
      if (res.ok) return;
    } catch {
      // not up yet
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error('hosted API did not become ready');
};

// Fail fast if the spawned API dies while the test is running.
const apiDeath = new Promise((_, reject) => {
  child.on('exit', (code) => reject(new Error(`hosted API exited (code ${code})`)));
});
const guard = async (work) => Promise.race([work, apiDeath]);

const latestLink = (marker) => {
  pollEmails();
  for (let i = emails.length - 1; i >= 0; i -= 1) {
    const body = emails[i].body ?? '';
    const at = body.indexOf(marker);
    if (at !== -1) {
      // The log line is JSON: newlines are literal backslash-n sequences.
      const token = body
        .slice(at + marker.length)
        .split(/[)\s"'\\]/)[0];
      // Rebuild the link against the web origin the run targets (the email
      // embeds AUTH_BASE_URL; assert it matches the origin under test).
      const path = marker.split('?')[0];
      return `${webBase}${path}?token=${token}`;
    }
  }
  return null;
};
const waitForLink = async (marker) => {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const link = latestLink(marker);
    if (link) return link;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`no email link containing ${marker}; ${emails.length} emails captured`);
};

let browser;
let failed = false;
try {
  await guard(waitForApi());
  browser = await chromium.launch({
    headless: true,
    ...(process.env.BROWSER_EXECUTABLE
      ? { executablePath: process.env.BROWSER_EXECUTABLE }
      : {}),
  });
  const context = await browser.newContext();
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  // --- 1. Security headers on the API through the same origin ------------
  const health = await page.request.get(`${webBase}/api/v1/health/live`);
  assert.ok(health.ok(), 'health endpoint reachable through the web origin');
  assert.match(health.headers()['content-security-policy'] ?? '', /default-src 'self'/, 'CSP enforced');
  // Frame protection: CSP frame-ancestors plus X-Frame-Options.
  assert.match(
    health.headers()['content-security-policy'] ?? '',
    /frame-ancestors 'self'/,
    'CSP frame-ancestors',
  );
  assert.equal(health.headers()['x-frame-options'], 'SAMEORIGIN', 'frame options');
  assert.ok(health.headers()['x-content-type-options'], 'nosniff');
  const requestId = health.headers()['x-request-id'];
  assert.ok(requestId && requestId.length >= 32, 'correlation id present');

  // --- 2. Register through the UI ----------------------------------------
  await page.goto(`${webBase}/account`);
  await page.getByRole('button', { name: 'Register as a new user' }).click();
  await page.getByLabel('Username').fill(tag);
  await page.getByLabel('Email').fill(email);
  await page.getByLabel('Password (at least 12 characters)').fill(password);
  await page.getByRole('checkbox').check();
  await page.getByRole('button', { name: 'Register', exact: true }).click();
  await page.getByText('Check your email for a verification link').waitFor({ timeout: 10_000 });

  // --- 3. Verify through the real email link; replay is rejected ---------
  const verifyLink = await waitForLink('/auth/verify?token=');
  assert.match(verifyLink, /^https?:\/\/[^\s'")\\]+$/, `malformed link: ${verifyLink}`);
  await page.goto(verifyLink);
  await page
    .getByText('Your email address is verified')
    .waitFor({ timeout: 10_000 })
    .catch(async (e) => {
      const body = await page.evaluate(() => document.body.innerText);
      throw new Error(
        `verify page state failed: ${body.slice(0, 700).replace(/\n/g, ' | ')} :: ${e.message.split('\n')[0]}`,
      );
    });
  await page.goto(verifyLink); // replay: token is single-use
  await page.getByText('no longer valid').waitFor({ timeout: 10_000 });

  // --- 4. Sign in through the UI; cookies and session --------------------
  await page.goto(`${webBase}/account`);
  await page.getByLabel('Username or email').fill(tag);
  await page.getByLabel('Password').fill(password);
  await page.getByLabel('Stay signed in').selectOption('90');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.getByText(`Signed in as`).waitFor({ timeout: 10_000 });

  // HttpOnly session cookie is invisible to the page; the CSRF cookie is not.
  const visibleCookies = await page.evaluate(() => document.cookie);
  assert.ok(!visibleCookies.includes('archaeodash_session'), 'session cookie is HttpOnly');
  assert.ok(visibleCookies.includes('archaeodash_csrf'), 'CSRF cookie is readable');
  // --- 5. CSRF enforcement in the real browser context -------------------
  // (page.request keeps its own jar and refuses Secure cookies over http;
  // the property under test is what an in-page attacker can do, so the
  // fetches run inside the page like real application code would.)
  const csrfResults = await page.evaluate(async () => {
    const put = (headers) =>
      fetch('/api/v1/preferences', {
        method: 'PUT',
        credentials: 'include',
        headers: { 'content-type': 'application/json', ...headers },
        body: JSON.stringify({ key: 'theme', value: 'dark' }),
      }).then((res) => res.status);
    const csrf = document.cookie
      .split(';')
      .map((pair) => pair.trim())
      .find((pair) => pair.startsWith('archaeodash_csrf='))
      ?.split('=')
      .slice(1)
      .join('=');
    return { without: await put({}), with: await put({ 'x-csrf-token': csrf ?? '' }) };
  });
  assert.equal(csrfResults.without, 403, 'write without the CSRF header is 403');
  assert.equal(csrfResults.with, 204, 'write with the CSRF header succeeds');
  const sessionGet = await page.request.get(`${webBase}/api/v1/auth/session`);
  assert.equal(sessionGet.status(), 200, 'session endpoint reachable');

  // --- 6. Password reset through the email link ---------------------------
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  try {
    await page
      .getByRole('button', { name: 'Forgot your password?' })
      .waitFor({ timeout: 10_000 });
  } catch (error) {
    const body = await page.evaluate(() => document.body.innerText);
    throw new Error(`after sign-out: ${body.slice(0, 700).replace(/\n/g, ' | ')}`);
  }
  await page.getByRole('button', { name: 'Forgot your password?' }).click();
  await page.getByLabel('Email').fill(email);
  await page.getByRole('button', { name: 'Send reset email' }).click();
  await page.getByText('If that address has an account').waitFor({ timeout: 10_000 });
  const resetLink = await waitForLink('/auth/reset?token=');
  await page.goto(resetLink);
  await page.getByLabel('New password (at least 12 characters)').fill(newPassword);
  await page.getByRole('button', { name: 'Update password' }).click();
  await page.getByText('Password updated').waitFor({ timeout: 10_000 });

  // Old password is dead, new password works.
  await page.goto(`${webBase}/account`);
  await page.getByLabel('Username or email').fill(tag);
  await page.getByLabel('Password').fill(password);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  try {
    await page.getByText('Sign-in failed').waitFor({ timeout: 10_000 });
  } catch (error) {
    const body = await page.evaluate(() => document.body.innerText);
    throw new Error(`old-password sign-in state: ${body.slice(0, 700).replace(/\n/g, ' | ')}`);
  }
  await page.getByLabel('Password').fill(newPassword);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  try {
    await page.getByText('Signed in as').waitFor({ timeout: 10_000 });
  } catch (error) {
    const body = await page.evaluate(() => document.body.innerText);
    throw new Error(`new-password sign-in state: ${body.slice(0, 700).replace(/\n/g, ' | ')}`);
  }

  // --- 7. Sign out everywhere revokes the session ------------------------
  await page.getByRole('button', { name: 'Sign out everywhere' }).click();
  await page.getByRole('button', { name: 'Sign in', exact: true }).waitFor({ timeout: 10_000 });

  assert.deepEqual(pageErrors, [], 'no uncaught page errors');
  console.log('auth-security e2e: all assertions passed');

  // Persist the transcript next to the other e2e artifacts.
  const outDir = join(import.meta.dirname, 'out');
  mkdirSync(outDir, { recursive: true });
  writeFileSync(
    join(outDir, `auth-security-${tag}.json`),
    JSON.stringify({ tag, email, requestId, headers: health.headers() }, null, 2),
  );
} catch (error) {
  failed = true;
  console.error('auth-security e2e FAILED:', error);
  try {
    writeFileSync(
      join(tmpdir(), `auth-e2e-${tag}-stderr.log`),
      readFileSync(logPath, 'utf8'),
    );
  } catch {
    // log unavailable
  }
} finally {
  await browser?.close();
  child.kill('SIGTERM');
  rmSync(workdir, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
