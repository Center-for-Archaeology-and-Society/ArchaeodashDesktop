# Interaction Log 2026-10-29

Up: [[Interaction_Log]]

- Login plan step 4, open audit items P4–P6 fixed: one shared resolver (`resolve_in_project`) refuses links that lead outside the project in the group store, imports and uploads; scans skip such links; transactions only write visible `.parquet` paths and uploads refuse hidden folders; uploads publish with a no-clobber hard link. Full Rust workspace, parity and DB-backed API tests pass on home2. Not committed. See [[Hosted_Multi_User_Hardening_2026-10-28]].
- Committed login step 4 (multi-user hardening: cross-user read fix, limits, storage quota, workspace removal on project delete) together with the P4–P6 path fixes and Vault notes, with `commit.sh` (no push).
