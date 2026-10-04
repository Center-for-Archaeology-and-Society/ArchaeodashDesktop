/**
 * Hosted account page (Section 10.1 client surface): login, registration with
 * the consent notice version, email-verification notice, password reset, and
 * session/logout. Rendered only when the transport exposes `auth` (hosted
 * HTTP mode); desktop mode has no hosted accounts.
 */
import { useEffect, useRef, useState, type FormEvent, type ReactElement } from 'react';
import { Link } from 'react-router';
import type { AuthService, SessionInfo } from '@archaeodash/client';

export interface AccountPageProps {
  /** Hosted-account service; absent in desktop (Tauri) mode. */
  readonly auth?: AuthService;
}

type Mode = 'signin' | 'register' | 'reset-request' | 'reset-confirm';

/** Maps stable error-envelope codes to user-facing text (uniform messages). */
function errorText(error: unknown): string {
  // TransportError carries the server's ErrorEnvelope; plain Errors fall
  // through to the generic message.
  const envelope =
    typeof error === 'object' && error !== null && 'envelope' in error
      ? (error as { envelope?: { code?: unknown } }).envelope
      : error;
  const code =
    typeof envelope === 'object' && envelope !== null && 'code' in envelope
      ? String(envelope.code)
      : '';
  switch (code) {
    case 'invalid_credentials':
    case 'http_401':
      return 'Sign-in failed: check your username/email and password.';
    case 'email_unverified':
      return 'Verify your email address before signing in.';
    case 'rate_limited':
      return 'Too many attempts. Please wait and try again later.';
    case 'invalid_username':
      return 'Usernames are 3–40 letters, digits, hyphens, or underscores.';
    case 'invalid_email':
      return 'Enter a valid email address.';
    case 'invalid_password':
      return 'Passwords must be at least 12 characters.';
    case 'invalid_consent_version':
      return 'The terms have changed. Reload the page and accept the current notice.';
    case 'invalid_token':
    case 'token_expired':
    case 'token_used':
      return 'That link is no longer valid. Request a new email.';
    default:
      return 'Something went wrong. Please try again.';
  }
}

export function AccountPage({ auth }: AccountPageProps): ReactElement {
  const [session, setSession] = useState<SessionInfo | null>(null);
  const [checked, setChecked] = useState(false);
  const [mode, setMode] = useState<Mode>('signin');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [consentVersion, setConsentVersion] = useState('');

  useEffect(() => {
    if (!auth) return;
    let cancelled = false;
    auth
      .session()
      .then((info) => {
        if (cancelled) return;
        setSession(info);
        setChecked(true);
      })
      .catch(() => {
        if (!cancelled) setChecked(true);
      });
    void auth
      .consent()
      .then((consent) => {
        if (!cancelled) setConsentVersion(consent.consent_version);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [auth]);

  if (!auth) {
    return (
      <section aria-labelledby="account-heading">
        <h1 id="account-heading">Account</h1>
        <p>Hosted accounts are only available in the web application. The desktop app keeps your data entirely on your machine.</p>
      </section>
    );
  }

  const submit = (
    action: (form: HTMLFormElement) => Promise<void>,
    options: { refreshSession?: boolean } = {},
  ) => {
    return (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = event.currentTarget;
      setBusy(true);
      setError('');
      setNotice('');
      action(form)
        .then(() => {
          // Only sign-in sets a session cookie; registration and reset
          // request/confirm keep the signed-out state.
          const refreshed =
            options.refreshSession && auth
              ? auth.session().then((info) => setSession(info))
              : Promise.resolve();
          return refreshed.then(() => {
            if (options.refreshSession) setMode('signin');
            form.reset();
          });
        })
        .catch((err: unknown) => setError(errorText(err)))
        .finally(() => setBusy(false));
    };
  };

  if (session?.authenticated) {
    return (
      <section aria-labelledby="account-heading">
        <h1 id="account-heading">Account</h1>
        <p>
          Signed in as <strong>{session.username}</strong>
          {session.email ? <> ({session.email})</> : null}
          {session.email_verified === false ? ' — email not yet verified' : ''}
        </p>
        {error && <p role="alert">{error}</p>}
        {notice && <p role="status">{notice}</p>}
        <p>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              setError('');
              auth
                .logout()
                .then(() => auth.session())
                .then((info) => setSession(info))
                .catch((err: unknown) => setError(errorText(err)))
                .finally(() => setBusy(false));
            }}
          >
            Sign out
          </button>{' '}
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              setError('');
              auth
                .logoutAll()
                .then(() => auth.session())
                .then((info) => setSession(info))
                .catch((err: unknown) => setError(errorText(err)))
                .finally(() => setBusy(false));
            }}
          >
            Sign out everywhere
          </button>
        </p>
        <p>
          Read the <Link to="/info/terms">Terms &amp; Conditions</Link> and{' '}
          <Link to="/info/privacy">Privacy Policy</Link>.
        </p>
      </section>
    );
  }

  return (
    <section aria-labelledby="account-heading">
      <h1 id="account-heading">Account</h1>
      {session?.authenticated ? null : !checked && <p role="status">Checking session…</p>}
      {checked && session?.authenticated ? null : (
        <>
          {mode === 'signin' && (
            <form
              aria-label="Sign in"
              onSubmit={submit(
                async (form) => {
                  const data = new FormData(form);
                  const remember = String(data.get('remember') ?? '');
                await auth.login({
                  identifier: String(data.get('identifier') ?? ''),
                  password: String(data.get('password') ?? ''),
                  ...(remember === '30' || remember === '90'
                    ? { rememberDays: Number(remember) }
                    : {}),
                });
                },
                { refreshSession: true },
              )}
            >
              <h2>Sign in</h2>
              <label>
                Username or email
                <input name="identifier" autoComplete="username" required />
              </label>
              <label>
                Password
                <input name="password" type="password" autoComplete="current-password" required />
              </label>
              <label>
                Stay signed in
                <select name="remember" defaultValue="">
                  <option value="">For this session only</option>
                  <option value="30">30 days</option>
                  <option value="90">90 days</option>
                </select>
              </label>
              <button type="submit" disabled={busy}>
                Sign in
              </button>{' '}
              <button type="button" onClick={() => setMode('register')}>
                Register as a new user
              </button>{' '}
              <button type="button" onClick={() => setMode('reset-request')}>
                Forgot your password?
              </button>
            </form>
          )}
          {mode === 'register' && (
            <form
              aria-label="Register"
              onSubmit={submit(async (form) => {
                const data = new FormData(form);
                await auth.register({
                  username: String(data.get('username') ?? ''),
                  email: String(data.get('email') ?? ''),
                  password: String(data.get('password') ?? ''),
                  consentVersion: consentVersion,
                });
                setNotice(
                  'Check your email for a verification link, then sign in.',
                );
              })}
            >
              <h2>Register</h2>
              <p>
                You must accept the notice and verify your email before your
                first sign-in. See the{' '}
                <Link to="/info/terms">Terms &amp; Conditions</Link> and{' '}
                <Link to="/info/privacy">Privacy Policy</Link>.
              </p>
              <label>
                Username
                <input
                  name="username"
                  autoComplete="username"
                  minLength={3}
                  maxLength={40}
                  required
                />
              </label>
              <label>
                Email
                <input name="email" type="email" autoComplete="email" required />
              </label>
              <label>
                Password (at least 12 characters)
                <input
                  name="password"
                  type="password"
                  autoComplete="new-password"
                  minLength={12}
                  required
                />
              </label>
              <label>
                <input name="consent" type="checkbox" required /> I agree to the
                Terms &amp; Conditions and the Privacy Policy
                {consentVersion ? <> (version {consentVersion})</> : null}
              </label>
              <button type="submit" disabled={busy || !consentVersion}>
                Register
              </button>{' '}
              <button type="button" onClick={() => setMode('signin')}>
                Back to sign in
              </button>
            </form>
          )}
          {mode === 'reset-request' && (
            <form
              aria-label="Request password reset"
              onSubmit={submit(async (form) => {
                const data = new FormData(form);
                await auth.requestPasswordReset(String(data.get('email') ?? ''));
                setNotice('If that address has an account, a reset email has been sent.');
                setMode('signin');
              })}
            >
              <h2>Reset your password</h2>
              <label>
                Email
                <input name="email" type="email" autoComplete="email" required />
              </label>
              <button type="submit" disabled={busy}>
                Send reset email
              </button>{' '}
              <button type="button" onClick={() => setMode('signin')}>
                Back to sign in
              </button>
            </form>
          )}
          {mode === 'reset-confirm' && (
            <form
              aria-label="Choose a new password"
              onSubmit={submit(async (form) => {
                const data = new FormData(form);
                const token = new URLSearchParams(window.location.search).get('token') ?? '';
                await auth.confirmPasswordReset({
                  token,
                  newPassword: String(data.get('password') ?? ''),
                });
                setNotice('Password updated. Sign in with the new password.');
                setMode('signin');
              })}
            >
              <h2>Choose a new password</h2>
              <label>
                New password (at least 12 characters)
                <input
                  name="password"
                  type="password"
                  autoComplete="new-password"
                  minLength={12}
                  required
                />
              </label>
              <button type="submit" disabled={busy}>
                Update password
              </button>{' '}
              <button type="button" onClick={() => setMode('signin')}>
                Back to sign in
              </button>
            </form>
          )}
          {error && <p role="alert">{error}</p>}
          {notice && <p role="status">{notice}</p>}
        </>
      )}
    </section>
  );
}

/**
 * Email-link landing pages. Verification and reset emails carry
 * `{AUTH_BASE_URL}/auth/verify?token=…` and `/auth/reset?token=…`; these
 * routes consume the token through the API and route the user onward.
 */

function useLinkToken(): string | null {
  const [search, setSearch] = useState(() =>
    typeof window === 'undefined' ? '' : window.location.search,
  );
  useEffect(() => {
    if (typeof window === 'undefined') return;
    const onPop = () => setSearch(window.location.search);
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, []);
  return new URLSearchParams(search).get('token');
}

export function VerifyEmailPage({ auth }: AccountPageProps): ReactElement {
  const token = useLinkToken();
  const [state, setState] = useState<'working' | 'done' | 'failed'>(
    token ? 'working' : 'failed',
  );
  // The token is single-use: StrictMode's double-invoked effect (and any
  // re-render) must not fire the verify call twice, or the second request
  // would consume... nothing — it would get 'invalid_token' and mask the
  // success. The ref guards one submission per page load.
  const verifyStarted = useRef(false);
  useEffect(() => {
    if (!auth || !token || verifyStarted.current) return;
    verifyStarted.current = true;
    auth
      .verify(token)
      .then(() => setState('done'))
      .catch(() => setState('failed'));
  }, [auth, token]);
  return (
    <section aria-labelledby="verify-heading">
      <h1 id="verify-heading">Email verification</h1>
      {!token && <p role="alert">This verification link is missing its token.</p>}
      {state === 'working' && <p role="status">Verifying your email address…</p>}
      {state === 'done' && (
        <p role="status">
          Your email address is verified. You can now{' '}
          <Link to="/account">sign in</Link>.
        </p>
      )}
      {state === 'failed' && token && (
        <p role="alert">
          That verification link is no longer valid.{' '}
          <Link to="/account">Sign in</Link> to request a new email.
        </p>
      )}
    </section>
  );
}

export function ResetPasswordPage({ auth }: AccountPageProps): ReactElement {
  const token = useLinkToken();
  const [state, setState] = useState<'form' | 'done'>('form');
  const [error, setError] = useState('');
  if (!token) {
    return (
      <section aria-labelledby="reset-heading">
        <h1 id="reset-heading">Choose a new password</h1>
        <p role="alert">This reset link is missing its token.</p>
        <p>
          <Link to="/account">Back to sign in</Link>
        </p>
      </section>
    );
  }
  if (state === 'done') {
    return (
      <section aria-labelledby="reset-heading">
        <h1 id="reset-heading">Choose a new password</h1>
        <p role="status">
          Password updated. You can now <Link to="/account">sign in</Link> with
          the new password.
        </p>
      </section>
    );
  }
  return (
    <section aria-labelledby="reset-heading">
      <h1 id="reset-heading">Choose a new password</h1>
      <form
        aria-label="Choose a new password"
        onSubmit={(event) => {
          event.preventDefault();
          if (!auth) return;
          const data = new FormData(event.currentTarget);
          const password = String(data.get('password') ?? '');
          setError('');
          auth
            .confirmPasswordReset({ token, newPassword: password })
            .then(() => setState('done'))
            .catch((err: unknown) => setError(errorText(err)));
        }}
      >
        <label>
          New password (at least 12 characters)
          <input
            name="password"
            type="password"
            autoComplete="new-password"
            minLength={12}
            required
          />
        </label>
        <button type="submit">Update password</button>
      </form>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
