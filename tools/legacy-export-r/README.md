# Legacy R Oracle

`capture_baselines.R` produces the Phase-0 legacy behavioral baselines in `fixtures/golden/` from the canonical `INAA_test.csv` fixture. It sources only the current pure R workflow functions and runs without a Shiny session.

Run it through the isolated `uvr` environment:

```bash
cd ~/.local/share/archaeodash-r-oracle/archaeodash-r-oracle
uvr run /home/rjbischo/ArchaeodashDesktop/tools/legacy-export-r/capture_baselines.R -- \
  --root /home/rjbischo/ArchaeodashDesktop \
  --seed 20260914
```

The command copies `inst/app/INAA_test.csv` to `fixtures/INAA_test.csv` only when the hashes differ, then captures the 14 procedures listed in `IMPLEMENTATION.md` §15.4. `fixtures/golden/manifest.json` records fixture, source-module, artifact, package, RNG, default-argument, and Git-revision metadata.

Generated fixture and golden files are baseline artifacts. Regenerate them through this command; do not edit them by hand.
