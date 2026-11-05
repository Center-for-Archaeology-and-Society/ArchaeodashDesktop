---
type: Decision
title: Multiplot pop-out windows show the plot first
description: Multiplot pop-out windows put the plot at the top and the controls, sampling note and legend below it.
status: stable
generated:
  by: claude-code/claude-opus-5-5
  at: 2026-11-05T18:10:00+00:00
sources:
  - id: window-tsx
    resource: ../apps/web/src/visualize/MultiplotWindow.tsx
  - id: styles
    resource: ../apps/web/src/styles.css
---

Up: [Index](Index.md)

# Multiplot pop-out windows show the plot first

Requested by the user on 2026-11-05: in pop-out windows, the plot comes first and everything else goes below it. This builds on [Multiplot pop-out window tiling](Multiplot_Popout_Window_Tiling_2026-11-05.md); tiled windows are small, so the plot starts at the top edge.

## Behavior
- **Single-plot window:** the plot comes first. Below it are the render mode, point size and save buttons, the sampling note (interactive mode) and the group legend.[^window-tsx] The plot's height is the window height minus where the plot starts, the measured height of the section below it, and a 32 px margin. It is recalculated when the window resizes or the render mode changes.
- **Full-grid window:** the grid of panels moves above the column pickers, controls, legend and progress line. This is done with CSS (`order: -1` in a flex column), only inside `.multiplot-window`.[^styles] The Multiplot view inside the main app is unchanged.

## Checks
- Web typecheck passed, and all 337 web tests passed on home2 (commit `def3593`).
- Deployed to the web app as `cas-def3593`. The live stylesheet contains the new `multiplot-window-below` rule.
- Desktop builds published from `def3593`; see [Desktop downloads on cas](Desktop_Download_On_Cas_2026-11-05.md).
- Not checked by eye in a browser or the desktop app, and there's no automated test of the visual order.

[^window-tsx]: [MultiplotWindow.tsx](../apps/web/src/visualize/MultiplotWindow.tsx)
[^styles]: [styles.css](../apps/web/src/styles.css)
