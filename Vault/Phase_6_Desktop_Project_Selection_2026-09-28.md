# Phase 6 desktop project selection

The Tauri shell now opens a project through the native folder picker. It builds a complete replacement set of project-scoped desktop services before swapping state, so picker cancellation or initialization failure preserves the active project. Successful switches increment a generation value, cancel old project jobs through service drop, stop old job watchers when their IDs disappear, and remount client route state even when reopening the same directory.

The browser client exposes project commands only on the Tauri transport. Desktop shell users can open or switch folders; cancel leaves the current route and project unchanged.

Related: [[Phase_6_Remaining_Controls_Jobs_2026-09-28]], [[Interaction_Log_2026-09-28]].
