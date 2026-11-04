import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir, mkdtemp, readFile, rm, stat, truncate, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {dirname, join} from 'node:path';
import {gzipSync} from 'node:zlib';
import {stageSite} from '../../scripts/deploy.mjs';

async function put(root, path, text = '') {
  await mkdir(dirname(join(root, path)), {recursive: true});
  await writeFile(join(root, path), text);
}

async function exists(path) {
  return stat(path).then(() => true, () => false);
}

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'deploy-'));
  t.after(() => rm(root, {recursive: true, force: true}));
  const web = join(root, 'web');
  const manifest = {content_base: 'assets/runtime/current/', file_catalog: {files: [{file: 'world/a.bc'}]}};
  await put(web, 'assets/reference/manifest.json.gz', gzipSync(JSON.stringify(manifest)));
  await put(web, 'index.html', '<html>');
  await put(web, 'pkg/the_long_slop_bg.wasm');
  await put(web, 'assets/runtime/current/world/a.bc', 'a');
  await put(web, 'assets/runtime/current/world/unused.bc', 'b');
  await put(web, 'assets/runtime/old/world/a.bc', 'c');
  await put(web, 'assets/runtime-tmp/world/a.bc', 'd');
  await put(web, 'assets/library/big.bin', 'e');
  return {web, site: join(root, 'site')};
}

test('stages the app and only the catalog files of the current package', async t => {
  const {web, site} = await fixture(t);
  const {large} = await stageSite({webRoot: web, siteRoot: site});
  assert.deepEqual(large, []);
  for (const path of ['index.html', 'pkg/the_long_slop_bg.wasm', 'assets/reference/manifest.json.gz', 'assets/runtime/current/world/a.bc']) {
    assert.ok(await exists(join(site, path)), path);
  }
  for (const path of ['assets/runtime/current/world/unused.bc', 'assets/runtime/old', 'assets/runtime-tmp', 'assets/library']) {
    assert.ok(!await exists(join(site, path)), path);
  }
  assert.match(await readFile(join(site, '_headers'), 'utf8'), /^\/assets\/runtime\/\*\n {2}Cache-Control: public, max-age=31536000, immutable/);
});

test('routes a file over the Pages size limit to R2', async t => {
  const {web, site} = await fixture(t);
  await truncate(join(web, 'pkg/the_long_slop_bg.wasm'), 25 * 1024 * 1024 + 1);
  const {large} = await stageSite({webRoot: web, siteRoot: site});
  assert.deepEqual(large.map(({path}) => path), ['pkg/the_long_slop_bg.wasm']);
  const keys = JSON.parse(await readFile(join(site, 'r2.json'), 'utf8'));
  assert.deepEqual(keys, {'/pkg/the_long_slop_bg.wasm': large[0].key});
  assert.match(large[0].key, /^[a-f0-9]{64}$/);
  assert.ok(!await exists(join(site, 'pkg/the_long_slop_bg.wasm')));
  const routes = JSON.parse(await readFile(join(site, '_routes.json'), 'utf8'));
  assert.deepEqual(routes.include, ['/pkg/the_long_slop_bg.wasm']);
});
