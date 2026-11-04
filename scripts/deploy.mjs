// Stages the built game for Cloudflare Pages and uploads it with Wrangler.
// The WASM packages get a content hash in their path, so browsers cache them as immutable.
// Files over the Pages size limit and the Brotli WASM files go to R2 under their SHA-256.
// `functions/[[path]].js` serves them on the same site.
// Each deployment carries its own `r2.json` map, so a dashboard rollback also restores its R2 files.
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import {cp, mkdir, readdir, readFile, rm, stat, writeFile} from 'node:fs/promises';
import {dirname, extname, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {gunzipSync} from 'node:zlib';
import {prepareStartupCompression, WASM} from './prepare-http-compression.mjs';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// Cloudflare Pages free plan limits.
const MAX_FILES = 20000;
const MAX_FILE_BYTES = 25 * 1024 * 1024;
const IMMUTABLE = ['assets/runtime/', 'pkg/'];
const HEADERS = IMMUTABLE.map(path => `/${path}*\n  Cache-Control: public, max-age=31536000, immutable\n`).join('');
const BUCKET = 'the-long-slop-large';
const SITE = 'https://the-long-slop.pages.dev/';
const TYPES = {'.wasm': 'application/wasm', '.json': 'application/json'};
const KEY = /^[a-f0-9]{64}$/;

const webPath = path => path.split(sep).join('/');
// The export library, old runtime packages and export evidence are not part of the site. `stagePackages` copies `pkg`.
// The game reads only `manifest.bin.gz` from `assets/reference`. `material-evidence.json` alone is over 25 MiB.
const excluded = path => path === 'assets/library' || path === 'assets/runtime' || path.startsWith('assets/runtime-') || path === 'pkg'
  || /^assets\/reference\/(([\w-]+-evidence|library-selection)\.json|manifest\.json\.gz)$/.test(path);

async function sha256(path) {
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(path)) hash.update(bytes);
  return hash.digest('hex');
}

async function* files(root) {
  for (const entry of await readdir(root, {withFileTypes: true})) {
    const path = resolve(root, entry.name);
    if (entry.isDirectory()) yield* files(path);
    else yield path;
  }
}

// Copies each wasm-bindgen module and its WASM to `pkg/<hash>/`. The hash covers both files.
// Rewrites the `./pkg/` imports of the app scripts to the new paths.
// Returns the source of each staged package file.
async function stagePackages(webRoot, siteRoot) {
  const sources = new Map();
  const paths = new Map();
  for (const name of await readdir(resolve(webRoot, 'pkg'))) {
    if (!name.endsWith('.js')) continue;
    const files = [name, name.replace(/\.js$/, '_bg.wasm')];
    const hash = createHash('sha256');
    for (const file of files) hash.update(await sha256(resolve(webRoot, 'pkg', file)));
    const folder = `pkg/${hash.digest('hex').slice(0, 16)}`;
    await mkdir(resolve(siteRoot, folder), {recursive: true});
    for (const file of files) {
      await cp(resolve(webRoot, 'pkg', file), resolve(siteRoot, folder, file));
      sources.set(`${folder}/${file}`, resolve(webRoot, 'pkg', file));
      paths.set(`./pkg/${file}`, `./${folder}/${file}`);
    }
  }
  for (const entry of await readdir(siteRoot, {withFileTypes: true})) {
    if (!entry.isFile() || !entry.name.endsWith('.js')) continue;
    const path = resolve(siteRoot, entry.name);
    const text = await readFile(path, 'utf8');
    await writeFile(path, text.replace(/\.\/pkg\/[\w.-]+/g, file => {
      if (!paths.has(file)) throw Error(`${entry.name} imports ${file}, which the build did not make.`);
      return paths.get(file);
    }));
  }
  return sources;
}

// Copies the app, the reference manifest, and the files of the current runtime package.
// Moves files over 25 MiB out of the site. Routes them and the WASM files to the R2 function.
// Returns the R2 uploads: site path, source and R2 key.
export async function stageSite({webRoot = resolve(projectRoot, 'web'), siteRoot = resolve(projectRoot, '.cache/deploy'),
  compressionRoot = resolve(projectRoot, '.cache/http-compression')} = {}) {
  const manifest = JSON.parse(gunzipSync(await readFile(resolve(webRoot, 'assets/reference/manifest.json.gz'))).toString('utf8'));
  const base = manifest.content_base;
  if (!/^assets\/runtime\/[a-zA-Z0-9_-]+\/$/.test(base) || !Array.isArray(manifest.file_catalog?.files) || !Array.isArray(manifest.file_catalog.packs)) {
    throw Error('Invalid runtime package catalog.');
  }
  await rm(siteRoot, {recursive: true, force: true});
  await cp(webRoot, siteRoot, {recursive: true, filter: source => !excluded(webPath(relative(webRoot, source)))});
  const packageRoot = resolve(webRoot, base);
  // Start packs are package files too. Each one stays under the Pages file limit.
  for (const {file} of [...manifest.file_catalog.files, ...manifest.file_catalog.packs]) {
    const source = resolve(packageRoot, file);
    if (typeof file !== 'string' || !source.startsWith(packageRoot + sep)) throw Error(`Invalid catalog file: ${file}`);
    const target = resolve(siteRoot, base, file);
    await mkdir(dirname(target), {recursive: true});
    await cp(source, target);
  }
  const sources = await stagePackages(webRoot, siteRoot);
  await writeFile(resolve(siteRoot, '_headers'), HEADERS);
  let count = 0;
  let bytes = 0;
  const uploads = [];
  // Site path to R2 keys. `raw` is set only when the file is not in the site.
  const routes = {};
  for await (const path of files(siteRoot)) {
    const {size} = await stat(path);
    bytes += size;
    if (size > MAX_FILE_BYTES) {
      const site = webPath(relative(siteRoot, path));
      // The R2 function marks its files immutable.
      if (!IMMUTABLE.some(prefix => site.startsWith(prefix))) throw Error(`${site} is over 25 MiB and has no content hash.`);
      const key = await sha256(path);
      uploads.push({path: site, source: sources.get(site) ?? resolve(webRoot, site), key});
      routes[`/${site}`] = {raw: key};
      await rm(path);
    } else count++;
  }
  if (count + 1 > MAX_FILES) throw Error(`${count + 1} files. The limit is ${MAX_FILES}.`);
  // Reuses the Brotli files of the build, or makes them after a fast build.
  const compression = await prepareStartupCompression({webRoot, compressionRoot});
  const staged = new Map([...sources].map(([site, source]) => [source, site]));
  for (const path of WASM) {
    const site = staged.get(resolve(webRoot, path));
    const brotli = compression.files[`/${path}`]?.brotli;
    if (!site || !KEY.test(brotli?.sha256 ?? '')) throw Error(`No staged Brotli file for ${path}.`);
    uploads.push({path: site, source: resolve(compressionRoot, brotli.file), key: brotli.sha256});
    routes[`/${site}`] = {...routes[`/${site}`], br: brotli.sha256};
  }
  await writeFile(resolve(siteRoot, 'r2.json'), JSON.stringify(routes));
  await writeFile(resolve(siteRoot, '_routes.json'), JSON.stringify({version: 1, include: Object.keys(routes), exclude: []}));
  return {count, bytes, uploads};
}

function wrangler(args) {
  const result = spawnSync('npx', ['wrangler', ...args], {cwd: projectRoot, stdio: 'inherit', shell: process.platform === 'win32'});
  if (result.status !== 0) process.exit(result.status ?? 1);
}

// The site holds The Long Drive files. Upload only when Cloudflare Access asks for a login.
async function requireAccess() {
  const response = await fetch(SITE, {redirect: 'manual'});
  const location = response.headers.get('location') ?? '';
  if (!/^https:\/\/[a-z0-9-]+\.cloudflareaccess\.com\//.test(location)) {
    throw Error(`${SITE} does not redirect to a Cloudflare Access login (HTTP ${response.status}). Fix Access before you upload.`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await requireAccess();
  const {count, bytes, uploads} = await stageSite();
  console.log(`Staged ${count} files and ${uploads.length} R2 files, ${(bytes / 1024 / 1024).toFixed(1)} MiB.`);
  for (const {path, source, key} of uploads) {
    wrangler(['r2', 'object', 'put', `${BUCKET}/${key}`, '--file', source,
      '--content-type', TYPES[extname(path)] ?? 'application/octet-stream', '--remote']);
  }
  wrangler(['pages', 'deploy', '--branch', 'main']);
}
