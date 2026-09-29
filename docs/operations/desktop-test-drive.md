# Desktop test drive

This is a short local smoke walkthrough for the native Tauri desktop app. It
uses the checked-in `fixtures/INAA_test.csv` and does not require an API
service or account. On first use, Cargo and pnpm may download the locked build
dependencies. The built app itself does not contact a hosted API.

## Prepare a disposable project

From any working directory, run:

```sh
/path/to/ArchaeodashDesktop/scripts/prepare-desktop-test-project.sh
```

The default project is `${TMPDIR:-/tmp}/archaeodash-desktop-test-drive`. To
choose a different destination, pass a new directory path. The setup refuses
to use an existing path. It copies the fixture into the new project and uses
the app's Rust import preview and commit services to create the initial group
files partitioned by `CORE`.

## Launch and walk through

Run the app from any directory:

```sh
/path/to/ArchaeodashDesktop/scripts/run-desktop.sh
```

In the app:

1. Choose **Open project** and select the prepared test project folder.
2. Open **Data Manager** from the navigation, choose `INAA_test.csv`, and review the preview. The
   prepared project already contains group files; you can still test a fresh
   import by selecting the fixture and committing to the suggested `groups`
   destination (existing group files are retained and new imports receive
   unused names).
3. Open the available groups and inspect their rows and elemental columns.
4. Open the analysis page, choose a small supported analysis such as PCA or
   hierarchical clustering, and review its plot/results.
5. Export measured data or an analysis result to a new file in the project.
6. Close and relaunch the app, reopen the same folder, and confirm that the
   groups and exported file remain available.

The scripts resolve paths relative to themselves, so the current shell
directory does not need to be the repository. The launcher checks for a
working pnpm executable at the version pinned by the lockfile, installs
JavaScript dependencies, builds the web assets, and starts the native app.
First launch may download Rust build dependencies; the running app does not
need an API service or network connection. Project data stays in the chosen
project folder.

## Initial test limits

This walkthrough checks basic project opening, import, analysis, export, and
reopen behavior on the current machine. It does not establish cross-platform
performance, large-dataset limits, packaging/installer behavior, or full
statistical parity. The initial desktop release still uses the native folder
picker for project selection. Native webview acceptance and performance gates
remain open in the [Phase 6 validation report](phase-6-validation-2026-09-28.md).
