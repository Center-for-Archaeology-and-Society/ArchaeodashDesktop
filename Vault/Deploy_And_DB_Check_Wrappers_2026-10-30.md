Up: [[Index]]

# Deploy and DB-check wrappers (2026-10-30)

Two fixed-command wrappers, so routine deploys and database tests need no
hand-written `ssh` commands (each of those asks for approval on every device).

## `scripts/home2/deploy-cas.sh`
- No arguments. Deploys the committed HEAD of the home2 repository; uncommitted
  changes are not deployed (the build context is `git archive HEAD`).
- Builds `archaeodash-hosted:cas-<short sha>` on home2, ships it to cas through
  homepc, sets `ARCHAEODASH_TAG` in `/srv/archaeodash/.env`, runs `up -d app`,
  waits for healthy, then checks the public URLs (exits non-zero if one fails).
- To run it without a prompt, add `"Bash(bash scripts/home2/deploy-cas.sh)"` to
  the allow list in `.claude/settings.local.json` (not done by the agent).
- See [[Cas_Hosted_Deployment_2026-10-29]] for the stack.

## `scripts/home2/check.sh db`
- Starts a throwaway `postgres:16-alpine` container on home2 (random password,
  loopback port), runs the `archaeodash-api` and `archaeodash-control-postgres`
  tests with `DATABASE_URL` set (one thread), then removes the container.
- Without it the DB-backed tests skip silently on home2. First used for
  [[Hosted_Project_Trash_2026-10-30]]: 15 + 76 tests pass.

## Also in this change
- `tests/parity/tests/group_profile.rs`: the blank-group test still expected
  blanks to be refused; since [[Interaction_Log_2026-10-30]] (blank cells import
  as `(blank)`) it checks the `(blank)` partition instead.
- Visualize says how many samples are not plotted because they have no value
  on an axis (per group), instead of dropping them silently.

## Later
- `scripts/home2/deploy-desktop-cas.sh` publishes the Windows desktop build; see [[Desktop_Download_On_Cas_2026-11-05]].
