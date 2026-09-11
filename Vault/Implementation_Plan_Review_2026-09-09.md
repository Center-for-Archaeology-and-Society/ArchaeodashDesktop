# Implementation Plan Review 2026-09-09

## Summary

- Reviewed [[../IMPLEMENTATION]] (Node/Rust migration blueprint) end-to-end and spot-verified its factual claims against the R source.
- Confirmed accurate: `rvals$pcaData` export bug, orphaned UMAP header, dead `subsetData`/`getCDA`, hard-coded PCA `[,1:4]` and LD1/LD2, zScore/log10 semantics, eligibility formula, Hotelling rounding, UI limits (100k multiplot, bins, k-means ranges), INAA default list, 1500-row/95% numeric inference, missing-plot bands, empty Risk Reduction Plan, missing QMD sources, manual-only Windows CI, undeclared `profvis` runtime dependency.
- Key findings: Calamine does not read CSV/TSV; §13.8 dependency list mixes declared and undeclared packages; two `analytical_uuid` encodings weakens fail-closed profiles; one-group-per-file makes frequent group reassignment a full rewrite+revalidation (throughput gap); "never copy sources" principle conflicts with hosted uploads; numerical-backend spike must move to Phase 0; no effort/sizing or desktop-first cutover option.
- Verdict: conditionally approve architecture; revise scope/schedule risks before Phase 0 exit.
- Full review written to [[../IMPLEMENTATION_REVIEW]].

## Related

- [[Node_Rust_Migration_Implementation_Plan_2026-09-08]]
- [[System_Architecture_ArchaeoDash]]
- [[Key_Technical_Risks_2026-02-18]]
- [[Quality_MOC]]
