import { HttpTransport } from '@archaeodash/client';

const transport = new HttpTransport('/api');

async function boot(): Promise<void> {
  const root = document.getElementById('root');
  if (!root) throw new Error('missing #root');
  try {
    const info = await transport.appInfo();
    root.textContent = `ArchaeoDash ${info.version} — ready (${info.transport})`;
  } catch {
    root.textContent = 'ArchaeoDash — API unavailable';
  }
}

void boot();
