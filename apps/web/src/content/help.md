# ArchaeoDash Help Guide

## About

ArchaeoDash facilitates the analysis of elemental data (for example portable X-ray fluorescence (PXRF) and Neutron Activation Analysis (NAA)). It includes tools for data management, ordination, clustering, distance-based matching, and group assignment.

ArchaeoDash was originally developed in R Shiny by Matthew Peeples and Andrew Upton and has been extensively redesigned by Robert Bischoff in consultation with Jeffrey Ferguson.

## Two Modes of Operation

ArchaeoDash runs in two modes:

- **Desktop mode** runs entirely on your computer. You open a project folder from your own storage; your project, group, and source files never leave your machine. No account is required.
- **Hosted mode (web)** runs in your browser. You sign in with an account so your projects are available across devices. Projects are stored as files in your hosted file store — the account database holds only your identity, security, and preference records, never your analytical data.

Group files are self-describing and remain complete on their own; edits are tracked as revisions, so files stay readable even outside the application. Group files keep the measured values exactly as imported. Imputation, ratios, transformations, PCA, UMAP, LDA, and clustering are recalculated whenever an analysis runs and are never written into group files; save or export a result when you want to keep it.

## Accounts and Sessions (hosted mode)

### Registration

Open the **Account** page, choose *Register as a new user*, and complete the form. You must accept the Terms & Conditions and Privacy Policy notice (the registration shows the current version) and verify your email from the message sent to you before your first sign-in. Verification links are single-use and expire within 24 hours. If you forget your password, use *Forgot your password?* to request a reset email; reset links are also single-use.

### Signing in

On the **Account** page enter your username or email and password. *Stay signed in* offers **For this session only**, **30 days**, or **90 days**. Use *Sign out* to end the session on the current browser, or *Sign out everywhere* to revoke all sessions on every device.

### Cookies

ArchaeoDash uses two strictly necessary cookies: a session cookie (HttpOnly, Secure) that authenticates each request and is invisible to page scripts, and a CSRF cookie that the application reads to prevent cross-site request forgery. No advertising, analytics, or third-party cookies are used. See the Privacy Policy for details.

## Projects

Desktop mode: use the project menu in the top bar to **Open Project**, switch to another folder, or reopen one of your **recent projects**. When the app starts it reopens the last project if its folder still exists. Everything ArchaeoDash writes lives inside that folder: group files (Parquet), saved Analysis setups, and a disposable cache under `.archaeodash/`.

Hosted mode: the **Projects** page lists your projects and files; select a project and refresh to see its groups.

## Data Manager

The Data Manager (left sidebar) lists the project's group files, organized in **folders**.

### Folders and groups

- A **folder** is one grouping of your samples. Each group is one file inside a folder, and a sample may appear at most once per folder.
- Analyses use one folder at a time — the folder with the filled circle (badge *In analyses*). Click the circle beside another folder to use it instead.
- Tick or untick groups to include or exclude them from analyses. **All**, **None**, **Invert**, and **Only selected** change the inclusion quickly; the view filter shows all, included, or excluded groups.
- Toolbar actions: **New folder…**, **Rename folder…** (the selected folder), **Rename group…** (one highlighted group and its file; it stays checked or unchecked for analyses; F2 renames whichever is focused), **Copy to…** / **Move to…** (copy or move whole groups into another folder, for example to start a new grouping of the same samples; you can also drag groups onto a folder to copy them, or hold Shift while dropping to move them), **Merge** (groups in the same folder), **Add column…** (add a descriptive column with a default value to every group in a folder; replacing an existing column asks for confirmation), and **Delete** (groups, or an empty folder).
- Every change is a journaled transaction: it is either applied to every affected file or to none, and a change is refused if a file changed since you loaded it.

### Import data

Use **Import data** and choose a source file (CSV, TSV, or XLSX) anywhere in the project folder; pick a sheet for workbooks. The wizard shows the column names after cleaning and lets you:

- choose the **ID column** (ANID by default when present) or automatic row IDs;
- choose the **group column**, or name a single group when the file has none;
- choose the **measured (elemental) columns** — numeric columns matching the standard INAA element list are preselected — and which other columns to **keep as descriptive** data;
- set how blank, zero, negative, and missing values are read;
- review the groups and file names that will be created, and the destination folder.

If groups from the same source (or with the same names) already exist, the wizard asks whether to import into a new folder, replace the existing groups, or **add the rows to the existing groups** (append). Appended rows get their own hidden identities; descriptive columns are aligned by name. Appending is refused when it would add measured columns an existing group does not have, or when the same file was already appended. Confirming writes one group file per group with the source file and its checksum recorded.

### Analysis setup

**Analysis setup → Configure…** decides what every analysis and plot uses. It is saved in the project and applied each time an analysis runs; group files keep their measured values.

- **Elements** — which measured columns analyses and plots use (*Select all* / *Clear*).
- **Fill missing values (imputation)** — none, random forest, predictive mean matching, or weighted predictive mean matching (see *Methods*). The seed makes the imputed values reproducible.
- **Ratio variables** — choose numerators and denominators; every numerator is paired with every denominator (name a single pair if you like). A preview shows the first rows of the raw ratios. **Append** adds the ratios to the selected elements; **Only** analyzes just the ratios.
- **Transform** — none, log10, natural log, or z-score.
- **Precompute** — PCA, UMAP, and LDA can be computed in the background after you save and whenever group files change, so Ordination and Visualize open quickly.
- **Saved setups** — *Save as…* stores the current setup under a name (replacing a name asks first); *Load* makes a saved setup the active one; *Delete* removes it. **Reset to defaults** returns to all elements with no imputation, ratios, or transform.

## Explore

The Dataset picker at the top of each analysis page chooses which included groups to analyze (all by default).

### Dataset

An editable table of the selected groups. Search across all visible cells and click a column heading to sort (again for descending, a third time to clear). Descriptive cells can be edited: edits save automatically when you leave a cell or press Enter; Escape cancels. Undo / Redo (Ctrl+Z / Ctrl+Shift+Z) reverse saved edits while Explore stays open with the same groups. A cell that could not be saved is highlighted in red with a Retry option. Measured values and the group column are read-only; move samples between groups instead (Visualize & Assign, Cluster, Probabilities, Euclidean Distance).

### Missing values

A bar chart and table of missing values per element, from the measured values (never imputed ones). Bands: **Good** ≤ 5%, **OK** ≤ 40%, **Bad** ≤ 80%, **Remove** above 80%.

### Univariate plots

A histogram of any element with 2–100 equal-width bins (default 30).

### Crosstabs

Counts of one field against another, or the mean, median, or standard deviation (choose one under *Summary*) of a numeric second field for each value of the first. Any column can be the first field.

### Compositional profile plot

One line per sample across the selected elements, colored by group.

## Visualize & Assign

- **Axes** — plot elements (or ratios), PCA scores (labelled with their variance), UMAP dimensions, or linear discriminants (LDA needs three or more groups). X and Y must differ.
- **Filter** — show only rows whose metadata field has the chosen values; blank cells appear as `(Missing)`. *Clear filter* shows everything again.
- **Colors** — viridis or the default (Dark 3) palette.
- **Data ellipse** — on by default at 0.90; the slider sets 0.50–0.99. These are data ellipses, not confidence ellipses: at 0.95 an ellipse encloses roughly 95% of a group's points. They are drawn like R's `stat_ellipse()` (robust multivariate-t fit) for groups with at least four points.
- **Symbols** — marker shapes by a metadata field (the group column by default); shapes repeat after ten values.
- **Point labels** — label points by any column (the sample ID by default). Hover shows the group, ID, label, symbol field, and coordinates.
- **Selecting and assigning** — drag a lasso or box around points; double-click clears. Changing the filter keeps only the selected points that are still shown. Choose an existing group or **New group…** and click **Assign** to move the selected samples. A new group name that matches an existing group is refused — pick that group instead.

### Multiplots

Choose **Multiplot** as the plot mode, then pick the **X** and **Y** columns. Panels are laid out with Y columns as rows and X columns as columns (a column is never plotted against itself). Set the point size (0.1–10) and the overall plot height (500–2000 px). *Static* draws every point; *Interactive* allows lasso selection but samples at most 100,000 points in total and says when it does. Large grids draw a few panels at a time; *Stop* pauses drawing.

### Saving plots

Every plot has **Save PNG**, **Save SVG**, and **Size…** (image width and height for interactive charts, and PNG resolution — 300 dpi by default).

## Ordination

### PCA

Explained variance (scree plot and table), the individuals plot colored by group or by quality of representation (cos²), the variables plot with arrows colored by contribution, and a contribution table summing each element's contribution over the first four components (or all of them when there are fewer than four). PCA scores can be exported.

### UMAP

A two-dimensional UMAP embedding with a fixed seed, so the same data give the same picture here and on other pages.

### LDA

Linear discriminant scores by group and the discriminant (vector) plot of each element's coefficients. LDA needs at least three groups. With a single discriminant, the page shows LD1 by group and the LD1 coefficients instead of a two-axis plot.

## Cluster

Choose the analysis source (elements/ratios, PCA scores, UMAP dimensions, or linear discriminants), then run diagnostics or fit a method:

- **Run cluster diagnostics** — within-cluster sum of squares (elbow) and average silhouette width for k-means or k-medoids, for 1 up to the *Diagnostic maximum k* (default 10).
- **Hierarchical** clustering with Euclidean, Manhattan, Minkowski, or maximum distance and average (default), complete, Ward.D, or Ward.D2 linkage, shown as a dendrogram cut into the chosen number of clusters.
- **Divisive (DIANA)**, **k-means** (cluster count, random starts, iterations, seed), and **k-medoids (PAM)**.

Result tables can be sorted and filtered; *Columns* hides columns and *Compact table* tightens the rows (both remembered). Tick rows to assign them: move them all to one existing or new group, or choose *Use analysis groups* to map each cluster to an existing or new group; the moves are applied together after you review them.

## Probabilities and Distances

Membership probabilities say how likely each sample is to belong to each reference group.

1. Choose the analysis source and, for PCA, how many components to use (the list shows each choice's variance and cumulative variance; all components by default).
2. Choose the ID column (only columns with a unique value per sample are offered).
3. Choose the reference groups. The group sizes table shows which groups are large enough; groups that are too small are disabled (see *Methods*). If no group is large enough, all groups are used and the page says so.
4. Choose **Hotelling probabilities** (with Mahalanobis fallback) or **Mahalanobis distances** and run.

The result table shows each sample's best group and whether its own group was part of the comparison. Tick samples and assign them to their best group, to an existing group, or to a new group.

## Euclidean Distance

For each sample, the nearest samples in the reference groups by Euclidean distance. Choose the source, components, ID column, the number of matches (default 10), and whether matches may come from the sample's own group (excluded by default). Each match is its own row: tick the match whose group you want and assign it, or use *Check the nearest match of every sample*.

## Methods

- **Log10 / natural log** — logarithm of each value, rounded to three decimals. Zeros, negative, and missing values have no logarithm and become 0, as in the original ArchaeoDash, so impute first when values are missing.
- **Z-score** — each value as a percent of its row total, then standardized per element (mean 0, standard deviation 1).
- **Ratios** — computed from the imputed measured values, before the transform; a zero or missing denominator gives a missing ratio.
- **Imputation** — uses the `mice` methods of the original app: *random forest* (10 trees), *predictive mean matching* (5 donors), and *weighted predictive mean matching* (midastouch). Imputation runs on the selected elements only. The same seed gives the same imputed values.
- **PCA** — computed on the analysis columns without rescaling (like R's `prcomp` defaults).
- **UMAP** — a port of R `umap`'s naive method with a fixed seed. Results are reproducible but not identical to an R run.
- **LDA** — like R `MASS::lda`; needs at least three groups.
- **Hotelling probabilities** — for each reference group, the probability from Hotelling's T² that a sample belongs to it, as a percent. A group is large enough when it has more samples than the larger of the number of variables and the number of groups, plus one. If the calculation fails for any group (missing values or a covariance that cannot be inverted), the whole table falls back to Mahalanobis distances, as in the original app, and the page says so.
- **Mahalanobis distances** — distance from each sample to each group centroid, scaled by the group's covariance.
- **Euclidean distance** — straight-line distance in the analysis columns; ties keep data order.

## Notes and Troubleshooting

- Desktop mode keeps everything on your machine: keep your own backups of project folders. Hosted mode keeps operational backups for disaster recovery only — this is not a substitute for your own copies (see the Privacy Policy and Terms).
- If an analysis returns no results, check that groups are included, that the Analysis setup selects elements the groups share, and that enough groups are selected (LDA needs three).
- If a save fails because the file changed since it was loaded, reload and re-apply the change; the revision check prevents silent overwrites.
- Long analyses keep running when you switch pages; the activity indicator in the top bar shows they are busy, and a notification appears when one fails or finishes.
- Desktop: **Info → Show log folder** opens the folder with the app's log files, useful when reporting a problem.
