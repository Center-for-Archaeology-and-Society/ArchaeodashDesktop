/** Client fixtures and a mock transport for component tests (Section 4). */
import type { AppInfo } from '@archaeodash/contracts';

export const fixtureAppInfo: AppInfo = {
  app: 'archaeodash',
  version: '0.1.0',
  transport: 'http',
  ready: true,
};
