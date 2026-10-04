/**
 * Route registry (Section 9.3): the legacy navbar order with stable paths.
 * Info is a dropdown menu in the legacy navbar; here it becomes an /info
 * parent with Help / Terms & Conditions / Privacy Policy child routes.
 */
export interface NavRoute {
  /** Legacy navbar label. */
  readonly label: string;
  /** URL path. */
  readonly path: string;
  /** Legacy Shiny tab id where one existed (parity cross-reference). */
  readonly legacyId?: string;
  readonly children?: readonly NavRoute[];
}

export const navRoutes: readonly NavRoute[] = [
  { label: 'Home', path: '/', legacyId: 'hometab' },
  { label: 'Explore', path: '/explore', legacyId: 'exploretab' },
  { label: 'Visualize & Assign', path: '/visualize', legacyId: 'visualizetab' },
  { label: 'Ordination', path: '/ordination', legacyId: 'ordinationtab' },
  { label: 'Cluster', path: '/cluster', legacyId: 'clustertab' },
  { label: 'Probabilities and Distances', path: '/probabilities' },
  { label: 'Euclidean Distance', path: '/euclidean' },
  { label: 'Account', path: '/account' },
  { label: 'Projects', path: '/projects' },
  {
    label: 'Info',
    path: '/info',
    children: [
      { label: 'Help', path: '/info/help' },
      { label: 'Terms & Conditions', path: '/info/terms' },
      { label: 'Privacy Policy', path: '/info/privacy' },
    ],
  },
];
