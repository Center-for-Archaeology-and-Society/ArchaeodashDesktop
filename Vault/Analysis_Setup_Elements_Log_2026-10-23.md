# Analysis setup: elements and transform (2026-10-23)

Up: [[Index]]

## What
- Data Manager sidebar has an **Analysis setup** section: a one-line summary ("7 of 9 elements · Log10") and **Configure…**, a dialog to choose the elements used in analyses and plots and a transform (None, Log10, Natural log, Z-score).
- Stage 1 of the Data Manager proposals evaluated on 2026-10-23 (next: legacy MICE imputation port, then precomputed PCA/UMAP/LDA in a disk cache). See [[Interaction_Log_2026-10-23]].

## Design
- Stored as one reserved transformation definition, `analysis_setup`, through the existing transformations service (desktop: `.archaeodash/transformations/`). Group files never hold transformed values (Section 5).
- `apps/web/src/analysis-setup/`: `setup.ts` (pure helpers: `analysisInput` intersects the selected elements with the columns the loaded groups share and only sends a transformation when values change) and `AnalysisSetupContext.tsx` (provider in `main.tsx`, reloads per project generation; pages without a provider behave as before).
- Pages that follow it:
  - Ordination PCA/UMAP/LDA: columns plus transformation, waiting for the setup to load so they compute once; shows "Analysis setup: …" and which selected elements are not in every selected group.
  - Visualize: ordination axes as above; element axes and the multiplot use the selected elements, with transformed values from `transformations.apply` over all loaded groups pooled.
  - Cluster / Probabilities / Euclidean: "Analysis setup (Data Manager)" is the default Transformation choice.
  - Explore: missing-profile, histogram, and compositional-profile plots use the selected elements (measured values).
- Backend: `ApplyTransformationRequest.extra_paths` pools files with `CombinedInput` before transforming, matching ordination requests (column-wise z-score sees every group).

## Legacy behaviour kept
- Log transforms round to three decimals and turn zeros, negatives, and missing values into 0 (counted); the dialog says so. Imputation (stage 2) runs before the transform.

## Verification
- Web: typecheck clean; 137/137 tests (setup helpers, dialog form, panel without provider).
- Rust: `apply_pools_extra_paths_before_transforming`; application/contracts/desktop/api tests pass; fmt and clippy clean.
- Browser (stubbed API): saving 7 of 9 elements + Log10 updates the summary; Ordination then sends one PCA request with the 6 shared selected elements and `log10`.
