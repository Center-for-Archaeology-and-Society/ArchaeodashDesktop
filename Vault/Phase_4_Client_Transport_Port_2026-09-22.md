# Phase 4 Client Transport Port — 2026-09-22

Related: [[Phase_4_Typed_Preferences_2026-09-21]], [[Implementation_Plan_Revision_And_Split_2026-09-09]]

## Summary

First Phase 4 client slice: with all Phase 4 backend items done through both
adapters (per [[Phase_4_Typed_Preferences_2026-09-21]]), the remaining Phase 4
work is the React client surface (Section 9). This slice landed the typed
contract DTOs and the dual-adapter transport port. Commit `fa60bba`.
Also landed first: toolchain fix `970943b` — this environment's Node 22.22.1
is compiled without TypeScript support (`ERR_NO_TYPESCRIPT`), so `node
--experimental-strip-types` fails; added `amaro`-backed
`scripts/ts-register.mjs`/`ts-hooks.mjs` loader and pointed all five TS
package test scripts at it (the interrupted session had added the `amaro`
dep but never wired it).

## What was built

- `packages/contracts/src/index.ts`: full snake_case DTO mirror of the Rust
  contracts — imports, files, groups/transactions, transformations,
  ordination (PCA/LDA/UMAP), explore (missing profile/histogram/crosstab
  tagged `kind`/compositional profile), exports, preferences. The only
  camelCase wire names are `PreferenceKey` values and TransformMethod
  `zScore`, matching the Rust serde attributes.
- `packages/client`: `Transport` port with service groups
  (imports/files/groups/transformations/ordination/explore/exports/
  preferences; jobs/projects land with later phases per Section 9.2);
  `HttpTransport` (relative `/api/v1`, ErrorEnvelope normalization with
  `http_<status>` fallback, raw-byte upload, bare-JSON-string group-validate
  body, query-param delete guards, 204 handling) and `TauriTransport`
  (injected `invoke` so the package does not depend on `@tauri-apps/api`;
  camelCase arg keys over snake_case Rust params; string rejections
  normalized to `tauri_error` envelopes).
- `contract-suite.ts`: the Section 9.2 "same behavioral suite against both
  adapters" harness — endpoint/command mapping, pass-through, and error
  normalization shared by `http.test.ts` and `tauri.test.ts`; payload-shape
  specifics (bare-string body, query params, raw bytes, camelCase invoke
  keys) asserted per adapter. 23 client tests total.

## Verification

- `pnpm -r typecheck`, `pnpm -r test` (27 tests), `pnpm -r build` green.
- Rust gates green: `cargo fmt --check`, `cargo clippy --workspace -- -D
  warnings` (CI's exact invocation; `--all-targets` flags pre-existing test
  `unwrap()`s under clippy 1.98 and is not the repo gate), 150 tests pass.
- Key API facts pinned from source: `POST /api/v1/groups/validate` takes a
  bare JSON string; `DELETE /api/v1/groups/{*path}` carries
  `expected_revision`/`confirm_path` as query params; `PUT /preferences`
  returns 204; desktop `save_transformation` takes the bare definition;
  exports return CSV as a JSON string inside `ExportResult`.

## Next

- Slice B: React app shell in `apps/web` (Section 9.3 routes, themes,
  responsive sidebar) — React deps not yet installed.
- Known backend gap for a later slice: no `open_project` Tauri command yet
  (desktop state is constructed closed).
