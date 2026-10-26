# Interaction Log 2026-10-26

Up: [[Interaction_Log]]

- Built the Windows app from 9574455 (R-parity gap closure and audit fixes, see [[R_Parity_Gap_Closure_2026-10-24]]) and installed it to `%LOCALAPPDATA%\Programs\ArchaeoDash\ArchaeoDash.exe`; the previous build is kept as `ArchaeoDash.previous.exe`. Not yet tried by hand.
- Raised the clustering row limit after a 29,706-row file was rejected: k-means fit/diagnostics and Euclidean matching now accept up to 100,000 rows (distances computed on the fly in parallel, no n x n matrix); PAM/hierarchical/DIANA keep the ~1,000-row dense cap with a clearer message. Tests, clippy and fmt pass on home2. See [[Clustering_Row_Limit_100k_2026-10-26]].
- Assign selected samples (Visualize): after creating a new group, the Target group now switches to it and the name box clears, so assigning more samples no longer hits the "already exists" error. Also applies when a multi-source move creates the group then fails part-way. Committed together with the clustering row-limit change; tests not run yet (by request).
