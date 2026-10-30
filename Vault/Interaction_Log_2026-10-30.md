Up: [[Interaction_Log]]

# Interaction log 2026-10-30

- Committed guest sessions (`cfe6205`) and deployed them to cas as `cas-cfe6205`; checked start, ping, end and the browser flow on the live site. See [[Hosted_Guest_Sessions_2026-10-29]].
- Asked what the Values dropdown on Probabilities and Distances means, then why saved setups were missing from it (pages stay mounted, so the list was read once). Removed the dropdown from Cluster, Probabilities and Euclidean Distance: they always use the Data Manager's active Analysis setup; other saved setups are loaded there. Committed and deployed to cas.
