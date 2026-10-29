# Interaction Log 2026-10-29

Up: [[Interaction_Log]]

- Login plan step 4, open audit items P4–P6 fixed: one shared resolver (`resolve_in_project`) refuses links that lead outside the project in the group store, imports and uploads; scans skip such links; transactions only write visible `.parquet` paths and uploads refuse hidden folders; uploads publish with a no-clobber hard link. Full Rust workspace, parity and DB-backed API tests pass on home2. Not committed. See [[Hosted_Multi_User_Hardening_2026-10-28]].
- Committed login step 4 (multi-user hardening: cross-user read fix, limits, storage quota, workspace removal on project delete) together with the P4–P6 path fixes and Vault notes, with `commit.sh` (no push).
- Deploy to cas.rc.asu.edu/archaeodash: added web sub-path support, sub-path email links, `smtp-local` email mode, `AUTH_TRUSTED_PROXIES`, and `deploy/cas/` (compose, env example, Apache snippet, backup script). Built `archaeodash-hosted:cas` and rehearsed it end to end on home2 behind httpd. cas not touched yet: SSH timed out (off the ASU network). Not committed. See [[Cas_Hosted_Deployment_2026-10-29]].
- Deployed to https://cas.rc.asu.edu/archaeodash/ with open registration (via homepc as SSH jump): `/srv/archaeodash` compose stack, image `cas-20261029`, Apache include in default-ssl.conf (backup in /root), nightly root-only backups to /mnt/storage/backups/archaeodash. Public URL and neighbouring apps return 200. Repo changes not committed. See [[Cas_Hosted_Deployment_2026-10-29]].
- Committed the cas deployment work (web sub-path, sub-path email links, `smtp-local`, `AUTH_TRUSTED_PROXIES`, `deploy/cas/`, Vault note) with `commit.sh` (no push).
