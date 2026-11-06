import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir, mkdtemp, readdir, readFile, rm, stat, truncate, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {dirname, join} from 'node:path';
import {randomBytes} from 'node:crypto';
import {brotliDecompressSync, gzipSync} from 'node:zlib';
import {stageSite} from '../../scripts/deploy.mjs';
import {WASM} from '../../scripts/prepare-http-compression.mjs';
import {onRequestGet} from '../../functions/[[path]].js';

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
  await put(web, 'assets/reference/manifest.bin.gz', 'm');
  await put(web, 'index.html', '<html>');
  await put(web, 'main.js', "import('./pkg/the_long_slop.js');fetch('./pkg/the_long_slop_bg.wasm');");
  for (const path of WASM) {
    await put(web, path, path);
    await put(web, path.replace(/_bg\.wasm$/, '.js'), path);
  }
  await put(web, 'pkg/the_long_slop.d.ts');
  await put(web, 'assets/runtime/current/world/a.bc', 'a');
  await put(web, 'assets/runtime/current/packs/start-0.gz', 'p');
  await put(web, 'assets/runtime/current/world/unused.bc', 'b');
  await put(web, 'assets/runtime/old/world/a.bc', 'c');
  await put(web, 'assets/runtime-tmp/world/a.bc', 'd');
  await put(web, 'assets/library/big.bin', 'e');
  await put(web, 'assets/reference/material-evidence.json', 'f');
  await put(web, 'assets/reference/library-selection.json', 'g');
  return {web, site: join(root, 'site'), compressionRoot: join(root, 'compression'), packageBrotliRoot: join(root, 'package-brotli')};
}

// The package folder in the staged main.js.
async function packageFolder(site) {
  return /\.\/(pkg\/[a-f0-9]{16})\/the_long_slop\.js/.exec(await readFile(join(site, 'main.js'), 'utf8'))?.[1];
}

test('stages the app and only the catalog files of the current package', async t => {
  const {web, site, compressionRoot, packageBrotliRoot} = await fixture(t);
  await stageSite({webRoot: web, siteRoot: site, compressionRoot, packageBrotliRoot});
  for (const path of ['index.html', 'assets/reference/manifest.bin.gz', 'assets/runtime/current/world/a.bc', 'assets/runtime/current/packs/start-0.gz']) {
    assert.ok(await exists(join(site, path)), path);
  }
  for (const path of ['assets/runtime/current/world/unused.bc', 'assets/runtime/old', 'assets/runtime-tmp', 'assets/library',
    'assets/reference/material-evidence.json', 'assets/reference/library-selection.json', 'assets/reference/manifest.json.gz']) {
    assert.ok(!await exists(join(site, path)), path);
  }
  const headers = await readFile(join(site, '_headers'), 'utf8');
  for (const path of ['assets/runtime', 'pkg']) assert.ok(headers.includes(`/${path}/*\n  Cache-Control: public, max-age=31536000, immutable\n`), path);
});

test('puts a content hash in the package paths', async t => {
  const {web, site, compressionRoot, packageBrotliRoot} = await fixture(t);
  await stageSite({webRoot: web, siteRoot: site, compressionRoot, packageBrotliRoot});
  const folder = await packageFolder(site);
  assert.equal(await readFile(join(site, 'main.js'), 'utf8'), `import('./${folder}/the_long_slop.js');fetch('./${folder}/the_long_slop_bg.wasm');`);
  assert.equal((await readdir(join(site, 'pkg'))).length, WASM.length);
  assert.deepEqual((await readdir(join(site, folder))).sort(), ['the_long_slop.js', 'the_long_slop_bg.wasm']);
  await writeFile(join(web, 'pkg/the_long_slop_bg.wasm'), 'new');
  await stageSite({webRoot: web, siteRoot: site, compressionRoot, packageBrotliRoot});
  assert.notEqual(await packageFolder(site), folder);
});

test('refuses an import of a missing package file', async t => {
  const {web, site, compressionRoot, packageBrotliRoot} = await fixture(t);
  await put(web, 'other-worker.js', "import init from './pkg/other_worker.js';");
  await assert.rejects(stageSite({webRoot: web, siteRoot: site, compressionRoot, packageBrotliRoot}), /other-worker\.js imports \.\/pkg\/other_worker\.js/);
});

test('routes the WASM files to R2 with Brotli and a file over the Pages size limit raw', async t => {
  const {web, site, compressionRoot, packageBrotliRoot} = await fixture(t);
  await truncate(join(web, 'pkg/the_long_slop_bg.wasm'), 25 * 1024 * 1024 + 1);
  const {uploads} = await stageSite({webRoot: web, siteRoot: site, compressionRoot, packageBrotliRoot});
  const path = `${await packageFolder(site)}/the_long_slop_bg.wasm`;
  assert.deepEqual(uploads[0], {path, source: join(web, 'pkg/the_long_slop_bg.wasm'), key: uploads[0].key});
  assert.equal(uploads.length, WASM.length + 1);
  for (const {key, source} of uploads) {
    assert.match(key, /^[a-f0-9]{64}$/);
    assert.ok(await exists(source), source);
  }
  const keys = JSON.parse(await readFile(join(site, 'r2.json'), 'utf8'));
  assert.equal(Object.keys(keys).length, WASM.length);
  assert.deepEqual(Object.keys(keys[`/${path}`]), ['raw', 'br']);
  for (const [route, entry] of Object.entries(keys)) {
    if (route === `/${path}`) continue;
    assert.deepEqual(Object.keys(entry), ['br']);
    assert.ok(await exists(join(site, route)), route);
  }
  assert.ok(!await exists(join(site, path)));
  const routes = JSON.parse(await readFile(join(site, '_routes.json'), 'utf8'));
  assert.deepEqual(routes.include, ['/pkg/*', '/assets/runtime/*', '/api/*']);
});

test('routes a runtime package file to R2 Brotli only when it shrinks by 10% or more', async t => {
  const {web, site, compressionRoot, packageBrotliRoot} = await fixture(t);
  await writeFile(join(web, 'assets/runtime/current/world/a.bc'), 'a'.repeat(4096));
  const options = {webRoot: web, siteRoot: site, compressionRoot, packageBrotliRoot};
  const {uploads} = await stageSite(options);
  const keys = JSON.parse(await readFile(join(site, 'r2.json'), 'utf8'));
  const upload = uploads.find(({path}) => path === 'assets/runtime/current/world/a.bc');
  assert.deepEqual(keys['/assets/runtime/current/world/a.bc'], {br: upload.key});
  assert.equal(brotliDecompressSync(await readFile(upload.source)).toString(), 'a'.repeat(4096));
  assert.ok(await exists(join(site, 'assets/runtime/current/world/a.bc')));
  await writeFile(join(web, 'assets/runtime/current/world/a.bc'), randomBytes(4096));
  await stageSite(options);
  assert.equal(JSON.parse(await readFile(join(site, 'r2.json'), 'utf8'))['/assets/runtime/current/world/a.bc'], undefined);
});

test('the function sends Brotli only to clients that accept it', async () => {
  const object = key => ({body: key, size: key.length, httpEtag: `"${key}"`,
    writeHttpMetadata: headers => headers.set('Content-Type', 'application/wasm')});
  const env = {
    LARGE_FILES: {get: async key => object(key)},
    ASSETS: {fetch: async url => String(url).endsWith('/r2.json')
      ? Response.json({'/pkg/a.wasm': {raw: 'raw', br: 'brotli'}, '/pkg/b.wasm': {br: 'small'}, '/assets/runtime/p/a.bc': {br: 'bc'}})
      : new Response('site')},
  };
  const get = (path, encoding) => onRequestGet({env, request: new Request(`https://site${path}`, {headers: {'Accept-Encoding': encoding}})});
  const brotli = await get('/pkg/a.wasm', 'gzip, deflate, br, zstd');
  assert.equal(brotli.headers.get('Content-Encoding'), 'br');
  assert.equal(await brotli.text(), 'brotli');
  const raw = await get('/pkg/a.wasm', 'gzip, br;q=0');
  assert.equal(raw.headers.get('Content-Encoding'), null);
  assert.equal(await raw.text(), 'raw');
  const site = await get('/pkg/b.wasm', 'gzip');
  assert.equal(await site.text(), 'site');
  assert.match(site.headers.get('Vary'), /Accept-Encoding/);
  const runtime = await get('/assets/runtime/p/a.bc', 'br');
  assert.equal(runtime.headers.get('Content-Encoding'), 'br');
  assert.equal(runtime.headers.get('Cache-Control'), 'public, max-age=31536000, immutable');
  assert.equal(await runtime.text(), 'bc');
  const rawPackage = await get('/assets/runtime/p/a.bc', 'gzip');
  assert.equal(rawPackage.headers.get('Content-Encoding'), null);
  assert.equal(await rawPackage.text(), 'site');
  assert.equal(await (await get('/assets/runtime/p/other.bc', 'br')).text(), 'site');
});

test('refuses a file over the Pages size limit without a content hash', async t => {
  const {web, site, compressionRoot, packageBrotliRoot} = await fixture(t);
  await put(web, 'big.bin');
  await truncate(join(web, 'big.bin'), 25 * 1024 * 1024 + 1);
  await assert.rejects(stageSite({webRoot: web, siteRoot: site, compressionRoot, packageBrotliRoot}), /big\.bin is over 25 MiB/);
});
