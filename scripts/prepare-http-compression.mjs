import {createHash, randomUUID} from 'node:crypto';
import {createReadStream, createWriteStream} from 'node:fs';
import {mkdir, readdir, readFile, rename, rm, stat, writeFile} from 'node:fs/promises';
import {dirname, resolve, sep} from 'node:path';
import {Transform} from 'node:stream';
import {pipeline} from 'node:stream/promises';
import {fileURLToPath} from 'node:url';
import {constants, createBrotliCompress} from 'node:zlib';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const WASM = ['pkg/the_long_slop_bg.wasm', 'pkg/simulation_worker_bg.wasm', 'pkg/mesh_worker_bg.wasm', 'pkg/crunch_worker_bg.wasm'];
const canonical = value => typeof value === 'string' && /^[a-zA-Z0-9_./ -]+$/.test(value)
  && !value.split('/').some(part => !part || part === '.' || part === '..' || part.trim() !== part);

function digestStream() {
  const hash = createHash('sha256');
  let bytes = 0;
  const stream = new Transform({transform(chunk, encoding, callback) {
    hash.update(chunk); bytes += chunk.length; callback(null, chunk);
  }});
  return {stream, result: () => ({size: bytes, sha256: hash.digest('hex')})};
}

const sameSource = (a, b) => a.size === b.size && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs;

async function sha256File(path) {
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(path)) hash.update(bytes);
  return hash.digest('hex');
}

// Reuse the Brotli file of an earlier build when the source bytes did not change.
async function reusable(sourcePath, before, previous, compressionRoot) {
  const sha256 = await sha256File(sourcePath);
  const brotli = previous.get(sha256);
  if (!brotli || !/^[a-f0-9]{64}\.br$/.test(brotli.file)) return;
  const compressed = await stat(resolve(compressionRoot, brotli.file)).catch(() => undefined);
  if (compressed?.size !== brotli.size) return;
  const after = await stat(sourcePath);
  if (!sameSource(before, after)) {
    throw Error('Compression source changed: ' + sourcePath);
  }
  return {source: {size: before.size, sha256, mtimeMs: before.mtimeMs, ctimeMs: before.ctimeMs}, brotli};
}

export async function prepareStartupCompression({webRoot = resolve(projectRoot, 'web'),
  compressionRoot = resolve(projectRoot, '.cache/http-compression')} = {}) {
  const manifest = JSON.parse(await readFile(resolve(webRoot, 'assets/reference/catalog.json'), 'utf8'));
  if (!/^assets\/runtime\/[a-zA-Z0-9_-]+\/$/.test(manifest.content_base)
    || !Array.isArray(manifest.file_catalog?.files)) throw Error('Invalid runtime package catalog.');
  const paths = new Set(WASM);
  for (const receipt of manifest.file_catalog.files) {
    if (!canonical(receipt.file)) throw Error('Invalid runtime receipt path.');
    if (receipt.file.endsWith('.json')) paths.add(manifest.content_base + receipt.file);
  }
  await mkdir(compressionRoot, {recursive: true});
  const previous = new Map();
  try {
    const old = JSON.parse(await readFile(resolve(compressionRoot, 'index.json'), 'utf8'));
    for (const entry of Object.values(old.files ?? {})) previous.set(entry.source.sha256, entry.brotli);
  } catch { /* No earlier index: compress every file. */ }
  const index = {version: 1, files: {}};
  for (const name of [...paths].sort()) {
    const sourcePath = resolve(webRoot, name);
    if (!sourcePath.startsWith(resolve(webRoot) + sep)) throw Error('Invalid compression source path.');
    const before = await stat(sourcePath);
    if (!before.isFile()) throw Error('Compression source is not a file: ' + name);
    const reused = await reusable(sourcePath, before, previous, compressionRoot);
    if (reused) {
      index.files['/' + name] = reused;
      continue;
    }
    const source = digestStream(), encoded = digestStream();
    const temporary = resolve(compressionRoot, randomUUID() + '.tmp');
    try {
      await pipeline(createReadStream(sourcePath), source.stream,
        createBrotliCompress({params: {
          [constants.BROTLI_PARAM_QUALITY]: 9,
          [constants.BROTLI_PARAM_MODE]: name.endsWith('.json') ? constants.BROTLI_MODE_TEXT : constants.BROTLI_MODE_GENERIC,
          [constants.BROTLI_PARAM_SIZE_HINT]: before.size,
        }}), encoded.stream, createWriteStream(temporary, {flags: 'wx'}));
      const after = await stat(sourcePath);
      if (!sameSource(before, after)) {
        throw Error('Compression source changed: ' + name);
      }
      const raw = source.result(), brotli = encoded.result();
      if (raw.size !== before.size) throw Error('Compression source length changed: ' + name);
      const file = brotli.sha256 + '.br';
      await rename(temporary, resolve(compressionRoot, file));
      index.files['/' + name] = {source: {...raw, mtimeMs: before.mtimeMs, ctimeMs: before.ctimeMs},
        brotli: {...brotli, file}};
    } finally {
      await rm(temporary, {force: true});
    }
  }
  const kept = new Set(Object.values(index.files).map(entry => entry.brotli.file));
  for (const file of await readdir(compressionRoot)) {
    if (file.endsWith('.br') && !kept.has(file)) await rm(resolve(compressionRoot, file), {force: true});
  }
  const temporary = resolve(compressionRoot, randomUUID() + '.tmp');
  try {
    await writeFile(temporary, JSON.stringify(index) + '\n', {flag: 'wx'});
    await rename(temporary, resolve(compressionRoot, 'index.json'));
  } finally {
    await rm(temporary, {force: true});
  }
  return index;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const index = await prepareStartupCompression();
  const entries = Object.values(index.files);
  console.log(JSON.stringify({files: entries.length,
    sourceBytes: entries.reduce((sum, entry) => sum + entry.source.size, 0),
    encodedBytes: entries.reduce((sum, entry) => sum + entry.brotli.size, 0)}));
}
