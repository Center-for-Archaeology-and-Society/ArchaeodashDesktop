import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { EligibleGroupPicker, GroupSizesTable } from './MembershipGroups.tsx';

const groups = [{ group: 'A', count: 9, eligible: true }, { group: 'B', count: 8, eligible: true }, { group: 'C', count: 2, eligible: false }];

test('picker preselects eligible groups and disables ineligible ones with counts', () => {
  const html = renderToStaticMarkup(<EligibleGroupPicker groups={groups} selected={null} featureCount={4} onChange={() => {}} />);
  assert.match(html, /<input type="checkbox" checked=""\/>A <span class="muted">\(n = 9\)/);
  assert.match(html, /<input type="checkbox" disabled=""\/>C <span class="muted">\(n = 2, too few\)/);
  assert.match(html, /more than 5 samples/);
  const some = renderToStaticMarkup(<EligibleGroupPicker groups={groups} selected={['B', 'C']} featureCount={4} onChange={() => {}} />);
  assert.match(some, /<input type="checkbox"\/>A /);
  assert.match(some, /<input type="checkbox" checked=""\/>B /);
});

test('with no eligible group the picker announces the all-groups fallback', () => {
  const html = renderToStaticMarkup(<EligibleGroupPicker groups={groups.map(g => ({ ...g, eligible: false }))} selected={null} featureCount={20} onChange={() => {}} />);
  assert.match(html, /role="status"[^>]*>No group has enough samples to be eligible; membership will use all groups/);
});

test('group sizes table lists every group with its count', () => {
  const html = renderToStaticMarkup(<GroupSizesTable groups={groups} column="Site" />);
  assert.match(html, /Group sizes/);
  assert.match(html, /<td>A<\/td><td class="num">9<\/td><td>Yes<\/td>/);
  assert.match(html, /<td>C<\/td><td class="num">2<\/td><td>No<\/td>/);
});
