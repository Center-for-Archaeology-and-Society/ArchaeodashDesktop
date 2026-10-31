import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { GroupRowDto, GroupRowsResponse } from '@archaeodash/client';
import { SampleIdPicker, setIdPreference, useSampleIds } from './SampleIdPicker.tsx';

const rows = (ids: (string | null)[]): GroupRowDto[] => ids.map((visible_id, i) => ({ analytical_uuid: `u${i}`, visible_id, legacy_rowid: String(i + 1), descriptive: [], elemental: [] }));
const groupFile = (column: string, ids: (string | null)[]): { data: GroupRowsResponse } => ({
  data: { path: 'g', revision_id: 'r', visible_id_column: column, legacy_rowid_column: 'legacy_rowid', descriptive_columns: [], elemental_columns: [], rows: rows(ids) },
});

function Probe({ files }: { files: { data: GroupRowsResponse }[] }) {
  const combined = { rows: files.flatMap(f => f.data.rows), groupOf: files.flatMap((f, i) => f.data.rows.map(() => `G${i + 1}`)) };
  const ids = useSampleIds(files, combined);
  return <><SampleIdPicker ids={ids} /><output>{ids.column}|{ids.ids.join(',')}</output></>;
}

test('one shared Sample IDs choice drives every page; automatic picks rowid for mixed ID columns', () => {
  const mixed = [groupFile('anid', ['AN1']), groupFile('Sample', ['S1'])];
  const auto = renderToStaticMarkup(<Probe files={mixed} />);
  assert.match(auto, /<option value="auto" selected="">Automatic: rowid<\/option><option value="visible">Sample ID<\/option><option value="rowid">rowid<\/option>/);
  assert.match(auto, /<output>rowid\|G1:1,G2:1<\/output>/);
  assert.match(auto, /role="note">The groups use different ID columns \(anid, Sample\), so rowid is used\./);
  try {
    setIdPreference('visible');
    const visible = renderToStaticMarkup(<Probe files={mixed} />);
    assert.match(visible, /<option value="visible" selected="">Sample ID<\/option>/);
    assert.match(visible, /<output>Sample ID\|AN1,S1<\/output>/);
    const same = renderToStaticMarkup(<Probe files={[groupFile('anid', ['AN1', 'AN2'])]} />);
    assert.match(same, /<option value="visible" selected="">Sample ID \(anid\)<\/option>/);
    assert.doesNotMatch(same, /role="note"/);
  } finally {
    setIdPreference('auto');
  }
});
