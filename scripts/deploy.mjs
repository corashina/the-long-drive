// Stages the built game for Cloudflare Pages and uploads it with Wrangler.
import {spawnSync} from 'node:child_process';
import {cp, mkdir, readdir, readFile, rm, stat, writeFile} from 'node:fs/promises';
import {dirname, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {gunzipSync} from 'node:zlib';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// Cloudflare Pages free plan limits.
const MAX_FILES = 20000;
const MAX_FILE_BYTES = 25 * 1024 * 1024;
const HEADERS = '/assets/runtime/*\n  Cache-Control: public, max-age=31536000, immutable\n';

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
    count++;
    bytes += size;
    if (size > MAX_FILE_BYTES) large.push(webPath(relative(siteRoot, path)));
  }
  if (large.length) throw Error(`Files over 25 MiB: ${large.join(', ')}`);
  if (count > MAX_FILES) throw Error(`${count} files. The limit is ${MAX_FILES}.`);
  return {count, bytes};
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const {count, bytes} = await stageSite();
  console.log(`Staged ${count} files, ${(bytes / 1024 / 1024).toFixed(1)} MiB.`);
  const project = process.env.PAGES_PROJECT ?? 'the-long-slop';
  const result = spawnSync('npx', ['wrangler', 'pages', 'deploy', '.cache/deploy', '--project-name', project, '--branch', 'main'],
    {cwd: projectRoot, stdio: 'inherit', shell: process.platform === 'win32'});
  process.exit(result.status ?? 1);
}
