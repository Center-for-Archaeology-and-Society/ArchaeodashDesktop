import type { Transport } from '@archaeodash/client';

/** The transport slice the Data Manager sidebar needs. */
export type DataManagerTransport = Pick<Transport, 'kind' | 'files' | 'imports' | 'groups'> & Partial<Pick<Transport, 'ordination'>>;
