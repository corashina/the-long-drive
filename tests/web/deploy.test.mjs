import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir, mkdtemp, readdir, readFile, rm, stat, truncate, writeFile} from 'node:fs/promises';
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
  const manifest = {content_base: 'assets/runtime/current/', file_catalog: {files: [{file: 'world/a.bc'}], packs: [{file: 'packs/start-0.gz'}]}};
  await put(web, 'assets/reference/manifest.json.gz', gzipSync(JSON.stringify(manifest)));
  await put(web, 'index.html', '<html>');
  await put(web, 'main.js', "import('./pkg/the_long_slop.js');fetch('./pkg/the_long_slop_bg.wasm');");
  await put(web, 'pkg/the_long_slop.js', 'js');
  await put(web, 'pkg/the_long_slop_bg.wasm');
  await put(web, 'pkg/the_long_slop.d.ts');
  await put(web, 'assets/runtime/current/world/a.bc', 'a');
  await put(web, 'assets/runtime/current/packs/start-0.gz', 'p');
  await put(web, 'assets/runtime/current/world/unused.bc', 'b');
  await put(web, 'assets/runtime/old/world/a.bc', 'c');
  await put(web, 'assets/runtime-tmp/world/a.bc', 'd');
  await put(web, 'assets/library/big.bin', 'e');
  return {web, site: join(root, 'site')};
}

// The package folder in the staged main.js.
async function packageFolder(site) {
  return /\.\/(pkg\/[a-f0-9]{16})\/the_long_slop\.js/.exec(await readFile(join(site, 'main.js'), 'utf8'))?.[1];
}

test('stages the app and only the catalog files of the current package', async t => {
  const {web, site} = await fixture(t);
  const {large} = await stageSite({webRoot: web, siteRoot: site});
  assert.deepEqual(large, []);
  for (const path of ['index.html', 'assets/reference/manifest.json.gz', 'assets/runtime/current/world/a.bc', 'assets/runtime/current/packs/start-0.gz']) {
    assert.ok(await exists(join(site, path)), path);
  }
  for (const path of ['assets/runtime/current/world/unused.bc', 'assets/runtime/old', 'assets/runtime-tmp', 'assets/library']) {
    assert.ok(!await exists(join(site, path)), path);
  }
  const headers = await readFile(join(site, '_headers'), 'utf8');
  for (const path of ['assets/runtime', 'pkg']) assert.ok(headers.includes(`/${path}/*\n  Cache-Control: public, max-age=31536000, immutable\n`), path);
});

test('puts a content hash in the package paths', async t => {
  const {web, site} = await fixture(t);
  await stageSite({webRoot: web, siteRoot: site});
  const folder = await packageFolder(site);
  assert.equal(await readFile(join(site, 'main.js'), 'utf8'), `import('./${folder}/the_long_slop.js');fetch('./${folder}/the_long_slop_bg.wasm');`);
  assert.deepEqual(await readdir(join(site, 'pkg')), [folder.slice(4)]);
  assert.deepEqual((await readdir(join(site, folder))).sort(), ['the_long_slop.js', 'the_long_slop_bg.wasm']);
  await writeFile(join(web, 'pkg/the_long_slop_bg.wasm'), 'new');
  await stageSite({webRoot: web, siteRoot: site});
  assert.notEqual(await packageFolder(site), folder);
});

test('refuses an import of a missing package file', async t => {
  const {web, site} = await fixture(t);
  await put(web, 'road-worker.js', "import init from './pkg/road_worker.js';");
  await assert.rejects(stageSite({webRoot: web, siteRoot: site}), /road-worker\.js imports \.\/pkg\/road_worker\.js/);
});

test('routes a file over the Pages size limit to R2', async t => {
  const {web, site} = await fixture(t);
  await truncate(join(web, 'pkg/the_long_slop_bg.wasm'), 25 * 1024 * 1024 + 1);
  const {large} = await stageSite({webRoot: web, siteRoot: site});
  const path = `${await packageFolder(site)}/the_long_slop_bg.wasm`;
  assert.deepEqual(large.map(({path, source}) => [path, source]), [[path, join(web, 'pkg/the_long_slop_bg.wasm')]]);
  const keys = JSON.parse(await readFile(join(site, 'r2.json'), 'utf8'));
  assert.deepEqual(keys, {[`/${path}`]: large[0].key});
  assert.match(large[0].key, /^[a-f0-9]{64}$/);
  assert.ok(!await exists(join(site, path)));
  const routes = JSON.parse(await readFile(join(site, '_routes.json'), 'utf8'));
  assert.deepEqual(routes.include, [`/${path}`]);
});

test('refuses a file over the Pages size limit without a content hash', async t => {
  const {web, site} = await fixture(t);
  await put(web, 'big.bin');
  await truncate(join(web, 'big.bin'), 25 * 1024 * 1024 + 1);
  await assert.rejects(stageSite({webRoot: web, siteRoot: site}), /big\.bin is over 25 MiB/);
});
