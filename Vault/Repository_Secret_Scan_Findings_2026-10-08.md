# Repository Secret Scan Findings 2026-10-08

Full-repository scan for sensitive information and secrets (working tree + full git history), requested as a security audit.

## Working tree: clean

- No `.env`/`.pem`/key/cert/credential files tracked; `.Renviron.example` contains placeholders only.
- `.gitignore` covers `.Renviron`, `.env.runtime`, and `db_backup_auth_cleanup_*` dumps.
- Deploy configs (`deploy/compose/hosted-stack.yml`, systemd, CI workflows) use env-var interpolation and local/test-only credentials (`postgres:test`, `nobody:nopass`); no hardcoded production secrets.
- Code matches for "password/token/secret" are schema definitions, env-var reads, or test fixtures (`correct horse battery staple`); `Vault/` notes contain no secrets.

## Git history: one finding — PURGED 2026-10-08

- `.Renviron` with real database credentials (host `10.126.24.122`, user `admin1`, database password redacted here) was committed in old-rev `678c9c1` ("adding database", early history) and removed in old-rev `4f5b120` ("updated description", 2024-05-16), which also added `.Renviron` to `.gitignore`. The credentials remained reachable in history via `git show <old-rev>:.Renviron`.
- **Remediated same day:** history rewritten with `git filter-repo --invert-paths --path .Renviron --path data/user-files` (484 commits rewritten, `be2ddce` → `cb4c520`), full-history grep for the credential now returns zero hits across all refs, `git fsck` clean. Backup bundle kept at `/tmp/opencode/archaeodash-backup/archaeodash-pre-rewrite.bundle` (delete once satisfied). Force-pushed to `origin/master` over SSH (HTTPS push was 403 for `bischrob`).
- Caveats: commit hashes before the rewrite are invalid in clones/forks; collaborators must re-clone or `git fetch && git reset --hard origin/master`. GitHub may retain unreachable old commits (cached SHA views, forks, PRs) — the DB credential should still be rotated/confirmed dead, and GitHub Support can be asked to expire cached views if the repo was ever exposed.
- Minor: e2e test file-store objects under `data/user-files/` were briefly committed in old-rev `b2dadb9` (removed in old-rev `9f2a50f`); contents are synthetic CSV fixtures, also purged in the rewrite.

## Related

- [[Interaction_Log_2026-10-08]]
