# ArchaeoDash Help Guide

## About

ArchaeoDash facilitates the analysis of elemental data (for example portable X-ray fluorescence (PXRF) and Neutron Activation Analysis (NAA)). It includes tools for data management, ordination, clustering, distance-based matching, and group assignment.

ArchaeoDash was originally developed in R Shiny by Matthew Peeples and Andrew Upton and has been extensively redesigned by Robert Bischoff in consultation with Jeffrey Ferguson.

## Two Modes of Operation

ArchaeoDash runs in two modes:

- **Desktop mode** runs entirely on your computer. You open a project folder from your own storage; your project, group, and source files never leave your machine. No account is required.
- **Hosted mode (web)** runs in your browser. You sign in with an account so your projects are available across devices. Projects are stored as files in your hosted file store — the account database holds only your identity, security, and preference records, never your analytical data.

Group files are self-describing and remain complete on their own; edits are tracked as revisions, so files stay readable even outside the application. Calculations such as PCA, UMAP, LDA, and clustering are ephemeral unless you explicitly save or export their results.

## Accounts and Sessions (hosted mode)

### Registration

Open the **Account** page, choose *Register as a new user*, and complete the form. You must accept the Terms & Conditions and Privacy Policy notice (the registration shows the current version) and verify your email from the message sent to you before your first sign-in. Verification links are single-use and expire within 24 hours. If you forget your password, use *Forgot your password?* to request a reset email; reset links are also single-use.

### Signing in

On the **Account** page enter your username or email and password. *Stay signed in* offers **For this session only**, **30 days**, or **90 days**. Use *Sign out* to end the session on the current browser, or *Sign out everywhere* to revoke all sessions on every device.

### Cookies

ArchaeoDash uses two strictly necessary cookies: a session cookie (HttpOnly, Secure) that authenticates each request and is invisible to page scripts, and a CSRF cookie that the application reads to prevent cross-site request forgery. No advertising, analytics, or third-party cookies are used. See the Privacy Policy for details.

## Import and Manage Data

### Projects and files

Desktop mode: click **Open Project** (or *Switch Project*) and choose a project folder. Your data lives in files inside that folder.

Hosted mode: your account's project catalog lists your files; select a project and refresh to see its groups.

### Import Data

Use **Import data** in the Data Manager and choose a source file (CSV, TSV, or XLSX). The import preview shows normalized names and types; you select the visible ID column, optional group column, measured numeric columns, and how rows should be grouped. Confirming creates one group file per group with provenance recorded.

### Data Selection and Preparation

- Select the descriptive/group column.
- Choose groups to include.
- Select element concentration columns (numeric columns only).
- Choose an imputation method if needed.
- Choose a transformation method if needed.

Confirm the selections to apply them. Most analyses require this step before they update. A loading indicator is shown while the update runs.

### Transformations

When you confirm selections, you are prompted to name the transformation:

- If left blank, a timestamp-based default name is used.
- Reusing an existing name overwrites that transformation.

Each saved transformation can include:

- Transformed selected data
- PCA results (if selected)
- UMAP results (if selected)
- LDA results (if selected)

Transformation behavior:

- Selecting a transformation in the *Transformations* dropdown loads it immediately.
- *Delete* removes the selected transformation.

Saved transformations are revision-checked project artifacts; the analysis views record which transformation produced each result.

If *Run LDA* is selected with fewer than three groups, the app shows a warning because LDA visualization requires at least three groups.

### Additional Data Manager Tools

- *Reset elements to original* restores element values to the original imported values.
- *Add new column* creates a new column with a default value.
- *Clear workspace* clears temporary in-session state; saved project files are unchanged.

## Save Data

Use the export section to download current data products. Exports are explicit: nothing is written to your project files unless you save or export it. In most cases, confirm selections first so exported tables reflect current settings.

## Explore

This tab has options for exploring the selected data.

### DATASET

Use the dataset selector to choose the analysis source (`elements`, `principal components`, `UMAP`, or `linear discriminants`).

### CROSSTABS

Produce cross-tabulated counts of two fields, or mean/median/SD of a numeric-convertible second field grouped by the first.

### UNIVARIATE PLOTS

Per-element histograms and missing-value counts from the original measured predictors.

### COMPOSITIONAL PLOT PROFILE

Compositional profile lines across ordered predictors, colored by current groups where available.

## VISUALIZE & ASSIGN

- Source selector; X/Y selectors with PCA variance labels; reject same axis.
- Lasso/box selection on plots; double-click clears; selected-row table shows ANID, metadata, and predictors.
- Metadata field/value filter with `(Missing)` normalization and clear action.
- Optional data ellipse (0.50–0.99), symbol metadata field, optional labels with ANID/sample ID/row-number fallback.
- Assignment to an existing group or a new group; changes are recorded with revision checks.

## ORDINATION

### PCA

Principal component analysis with explained-variance reporting; use the scores as analysis sources elsewhere.

### LDA

Linear discriminant analysis against a chosen grouping column.

## CLUSTER

Cluster methods available:

- Optimal cluster count diagnostics (elbow and silhouette)
- Hierarchical agglomerative clustering (Ward, complete, average, single)
- Hierarchical divisive clustering (DIANA)
- K-means
- K-medoids (PAM)

Cluster analysis can be run using:

- elements
- principal components (PCA)
- UMAP
- linear discriminants (LDA)

After running a method, record cluster assignments back into a group file with the output column name of your choice; overwriting an existing column prompts for confirmation.

## PROBABILITIES AND DISTANCES

This tab includes group size summaries and a full membership probabilities table.

### Membership Probabilities Workflow

1. Choose the analysis source (`elements`, `principal components`, `UMAP`, or `linear discriminants`).
2. Choose the reference groups.
3. Compute membership probabilities; export if you want to keep them.

### Updating Group Assignments from Membership Probabilities

Reviewed assignments can be recorded back to group files through the explicit destination-mapping step; the app shows the source revision so writes cannot silently overwrite changed files.

## EUCLIDEAN DISTANCE

Compute Euclidean distance matches between analytical units and reference groups, with matches per analytical unit configurable.

## Notes and Troubleshooting

- Desktop mode keeps everything on your machine: keep your own backups of project folders. Hosted mode keeps operational backups for disaster recovery only — this is not a substitute for your own copies (see the Privacy Policy and Terms).
- If an analysis returns no results, verify that required inputs are selected and that selections were confirmed after recent data or option changes.
- If a save fails because the source file changed since it was loaded, reload the file and re-apply the change; the revision check prevents silent overwrites.
