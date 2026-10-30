Up: [[Interaction_Log]]

# Interaction log 2026-10-30

- Committed guest sessions (`cfe6205`) and deployed them to cas as `cas-cfe6205`; checked start, ping, end and the browser flow on the live site. See [[Hosted_Guest_Sessions_2026-10-29]].
- Asked what the Values dropdown on Probabilities and Distances means, then why saved setups were missing from it (pages stay mounted, so the list was read once). Removed the dropdown from Cluster, Probabilities and Euclidean Distance: they always use the Data Manager's active Analysis setup; other saved setups are loaded there. Committed and deployed to cas.
- Import no longer fails on a blank group cell: blanks in the group column become the group `(blank)` (stored value too), and the import preview and result warn how many rows. Committed and deployed to cas.
- Multiplot pickers could not take one element or a range while the grid was drawing. Picker input now stops drawing at once and the grid follows once the pickers settle; drawn panels are memoized. Plot mode is disabled unless Axes is Elements. Added "Open in new window" (all plots in one window, or one window per plot) for web and desktop. Committed and deployed to cas. See [[Multiplot_Responsive_Pickers_And_Popout_Windows_2026-10-30]].
