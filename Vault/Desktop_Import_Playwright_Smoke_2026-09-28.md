# Desktop import Playwright smoke — 2026-09-28

Added `scripts/e2e/import.mjs` as a reproducible browser smoke for the Data Manager. It uploads a synthetic CSV through the actual file picker, previews and imports two groups selected by a source column with ANID as the visible ID and Ca/Fe selected as measured columns, then imports the same rows into one named group. It also checks blank group-name validation, Cluster dataset availability, absence of analytical UUIDs in the rendered UI, and browser page errors.

Verification used an isolated disposable project with the loopback API on port 8787 and Vite on port 4173. `node scripts/e2e/import.mjs` passed all eight reported cases in 665 ms. `pnpm --filter @archaeodash/web build` also completed; Vite emitted its existing warning about the large Plotly chunk. Both services were stopped after the run.

Related: [[Initial_Desktop_Test_Drive_2026-09-28]]
