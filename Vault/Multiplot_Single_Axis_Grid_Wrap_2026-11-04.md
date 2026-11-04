Up: [[Index]]

# Multiplot: wrap single-axis panels into a grid (2026-11-04)

Follows [[Multiplot_Responsive_Pickers_And_Popout_Windows_2026-10-30]].

## Problem

With one X column (or one Y column) the multiplot drew every panel in a
single column (or row), which wastes the page and is hard to scan.

## Change (`apps/web/src/visualize/`)

- `gridShape(xCount, yCount, pairCount)` in `multiplot-model.ts`: when either
  axis has one column, panels wrap into a near-square grid
  (`ceil(sqrt(n))` columns). Otherwise the grid stays Y rows by X columns.
- `Multiplot.tsx` uses the shape for the CSS grid columns, the panel height
  (figure height / rows), the saved PNG/SVG layout, and the "all plots in one
  window" pop-out width. A wrapped grid skips the X = Y blank cell.
- Tests: `gridShape` cases and a component test for one X / one Y column.
