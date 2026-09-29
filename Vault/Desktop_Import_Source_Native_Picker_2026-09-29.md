# Desktop import source native picker

The Data Manager calls an optional `FilesService.pickImportSource` capability for Tauri. The Rust command snapshots the active project root and generation, opens the GTK picker with `.set_directory(project_root)` on every call, stages the selected CSV/TSV/XLSX file under that project, and returns the same `StagedFile` used by the browser upload flow. It rejects files over the existing 256 MiB source limit and refuses to stage if the active project changed while the dialog was open. The shared preview setup reports the selected filename and shows its staged project path. Browser transport keeps its normal file input.

Picker cancellation returns without replacing the current preview or showing an error. The busy indicator is cleared in the normal `finally` path. Tauri IPC invokes `pick_and_upload_source` without arguments.

Coverage checks the desktop picker affordance, browser file input fallback, Tauri cancellation mapping, and allowed/invalid source names. The Tauri command compiles; its focused Rust source-name test passes. Client/web typechecks and 94 web tests pass through the cached pnpm 12.4.1 module. The native picker initial-directory option is compiled but was not re-opened in the final picker change on a live window.

Related: [[Phase_6_Desktop_Project_Selection_2026-09-28]], [[Interaction_Log_2026-09-29]].
