// Stages the built game for Cloudflare Pages and uploads it with Wrangler.
// Files over the Pages size limit go to R2. `functions/[[path]].js` serves them on the same site.
import {spawnSync} from 'node:child_process';
import {cp, mkdir, readdir, readFile, rm, stat, writeFile} from 'node:fs/promises';
import {dirname, extname, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {gunzipSync} from 'node:zlib';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// Cloudflare Pages free plan limits.
const MAX_FILES = 20000;
const MAX_FILE_BYTES = 25 * 1024 * 1024;
const HEADERS = '/assets/runtime/*\n  Cache-Control: public, max-age=31536000, immutable\n';
const BUCKET = 'the-long-slop-large';
const SITE = 'https://the-long-slop.pages.dev/';
const TYPES = {'.wasm': 'application/wasm', '.json': 'application/json'};

const webPath = path => path.split(sep).join('/');
// The export library and old runtime packages are not part of the site.
const excluded = path => path === 'assets/library' || path === 'assets/runtime' || path.startsWith('assets/runtime-');

async function* files(root) {
  for (const entry of await readdir(root, {withFileTypes: true})) {
    const path = resolve(root, entry.name);
    if (entry.isDirectory()) yield* files(path);
    else yield path;
  }
}

// Copies the app, the reference manifest, and the files of the current runtime package.
// Moves files over 25 MiB out of the site and routes them to the R2 function. Returns their site paths.
export async function stageSite({webRoot = resolve(projectRoot, 'web'), siteRoot = resolve(projectRoot, '.cache/deploy')} = {}) {
  const manifest = JSON.parse(gunzipSync(await readFile(resolve(webRoot, 'assets/reference/manifest.json.gz'))).toString('utf8'));
  const base = manifest.content_base;
  if (!/^assets\/runtime\/[a-zA-Z0-9_-]+\/$/.test(base) || !Array.isArray(manifest.file_catalog?.files)) {
    throw Error('Invalid runtime package catalog.');
  }
  await rm(siteRoot, {recursive: true, force: true});
  await cp(webRoot, siteRoot, {recursive: true, filter: source => !excluded(webPath(relative(webRoot, source)))});
  const packageRoot = resolve(webRoot, base);
  for (const {file} of manifest.file_catalog.files) {
    const source = resolve(packageRoot, file);
    if (typeof file !== 'string' || !source.startsWith(packageRoot + sep)) throw Error(`Invalid catalog file: ${file}`);
    const target = resolve(siteRoot, base, file);
    await mkdir(dirname(target), {recursive: true});
    await cp(source, target);
  }
  await writeFile(resolve(siteRoot, '_headers'), HEADERS);
  let count = 0;
  let bytes = 0;
  const large = [];
  for await (const path of files(siteRoot)) {
    const {size} = await stat(path);
    bytes += size;
    if (size > MAX_FILE_BYTES) {
      large.push(webPath(relative(siteRoot, path)));
      await rm(path);
    } else count++;
  }
  if (count > MAX_FILES) throw Error(`${count} files. The limit is ${MAX_FILES}.`);
  // Pages needs one route. `/r2/*` matches no site file.
  const include = large.length ? large.map(path => `/${path}`) : ['/r2/*'];
  await writeFile(resolve(siteRoot, '_routes.json'), JSON.stringify({version: 1, include, exclude: []}));
  return {count, bytes, large};
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
  const {count, bytes, large} = await stageSite();
  console.log(`Staged ${count} files and ${large.length} R2 files, ${(bytes / 1024 / 1024).toFixed(1)} MiB.`);
  for (const path of large) {
    wrangler(['r2', 'object', 'put', `${BUCKET}/${path}`, '--file', resolve(projectRoot, 'web', path),
      '--content-type', TYPES[extname(path)] ?? 'application/octet-stream', '--remote']);
  }
  wrangler(['pages', 'deploy', '--branch', 'main']);
}
