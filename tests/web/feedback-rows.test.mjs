import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';

// Every HUD row needs a layout destination, or createFeedback throws at load.
test('every feedback row has a destination', async () => {
  const source = await readFile(new URL('../../web/feedback.js', import.meta.url), 'utf8');
  const rows = source.slice(source.indexOf('const rows=['), source.indexOf('].map(([bit,role'));
  const roles = [...rows.matchAll(/^\s*\[[^,\]]+,'(\w+)'/gm)].map(m => m[1]);
  const map = source.match(/const destinations=\{([^}]*)\}/)[1];
  const keys = new Set([...map.matchAll(/(\w+):'\w+'/g)].map(m => m[1]));
  assert.ok(roles.length > 30);
  assert.deepEqual(roles.filter(r => !keys.has(r)), []);
});
