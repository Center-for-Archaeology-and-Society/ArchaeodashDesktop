# Phase 6 procedure 13 browser render — 2026-09-29

The procedure 13 multiplot sampling budget now follows the actual 12 ordered
pairs produced by four numeric columns. The previous square column budget
under-sampled the plot. A component regression covers 15,000 rows in five
groups and checks 180,000 candidate points, 99,960 selected points, and 1,666
rows per group/facet.

An opt-in Playwright benchmark imports a generated 15,000-row fixture through
the local API, opens the merged dataset in the web app, switches to interactive
multiplot, waits for all 12 Plotly panels, and inspects the live trace arrays.
It verifies 8,330 points per panel and 99,960 in total, and rejects browser page
errors. The benchmark documents API, Vite preview, and Chromium setup in the
Phase 6 validation report. It records timing without a portable performance
threshold.

Two local runs completed in 3,175 ms and 3,045 ms from selecting multiplot until Plotly
completion on Linux x64, Node v22.22.1, and headless Chromium 151.0.7922.34.
Timing includes the initial static SVG mount and switch to interactive mode;
import, merge, navigation, and initial page/data loading were outside the timed
interval. These are local observations, not a cross-platform acceptance result.

Related: [[Phase_6_Cluster_Plots_2026-09-28]], [[Phase_5_Interactive_Multiplot_Plotly_2026-09-24]].
