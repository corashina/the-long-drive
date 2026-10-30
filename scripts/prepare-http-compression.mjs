import {createHash, randomUUID} from 'node:crypto';
import {createReadStream, createWriteStream} from 'node:fs';
import {mkdir, readFile, rename, rm, stat, writeFile} from 'node:fs/promises';
import {dirname, resolve, sep} from 'node:path';
import {Transform} from 'node:stream';
import {pipeline} from 'node:stream/promises';
import {fileURLToPath} from 'node:url';
import {constants, createBrotliCompress} from 'node:zlib';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
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

export async function prepareStartupCompression({webRoot = resolve(projectRoot, 'web'),
  compressionRoot = resolve(projectRoot, '.cache/http-compression')} = {}) {
  const manifest = JSON.parse(await readFile(resolve(webRoot, 'assets/reference/manifest.json'), 'utf8'));
  if (!/^assets\/runtime\/[a-zA-Z0-9_-]+\/$/.test(manifest.content_base)
    || !Array.isArray(manifest.file_catalog?.files)) throw Error('Invalid runtime package catalog.');
  const paths = new Set(['pkg/the_long_slop_bg.wasm', 'pkg/simulation_worker_bg.wasm', 'pkg/terrain_worker_bg.wasm', 'pkg/road_worker_bg.wasm', 'pkg/grass_worker_bg.wasm', 'assets/reference/manifest.json']);
  for (const receipt of manifest.file_catalog.files) {
    if (!canonical(receipt.file)) throw Error('Invalid runtime receipt path.');
    if (receipt.file.endsWith('.json')) paths.add(manifest.content_base + receipt.file);
  }
  await mkdir(compressionRoot, {recursive: true});
  const index = {version: 1, files: {}};
  for (const name of [...paths].sort()) {
    const sourcePath = resolve(webRoot, name);
    if (!sourcePath.startsWith(resolve(webRoot) + sep)) throw Error('Invalid compression source path.');
    const before = await stat(sourcePath);
    if (!before.isFile()) throw Error('Compression source is not a file: ' + name);
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
      if (before.size !== after.size || before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs) {
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
