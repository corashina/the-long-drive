import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { createReadStream, readFileSync } from 'node:fs';
import { open, realpath, stat } from 'node:fs/promises';
import { dirname, extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const types = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.wasm', 'application/wasm'],
  ['.json', 'application/json'],
  ['.gltf', 'model/gltf+json'],
  ['.bin', 'application/octet-stream'],
  ['.png', 'image/png'],
  ['.wav', 'audio/wav'],
  ['.ttf', 'font/ttf'],
  ['.otf', 'font/otf'],
  ['.glb', 'model/gltf-binary'],
]);
const hashPattern = /^[a-f0-9]{64}$/;
const eligible = path => ['.wasm', '.json'].includes(extname(path));
const sameSource = (a, b) => a.size === b.size && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs;
const fingerprint = file => `${file.size}:${file.mtimeMs}:${file.ctimeMs}:${file.dev}:${file.ino}`;

function encodingQualities(header = '') {
  const values = new Map();
  for (const part of header.split(',')) {
    const match = /^\s*([!#$%&'*+.^_`|~\w-]+)\s*(?:;\s*q\s*=\s*(0(?:\.\d{0,3})?|1(?:\.0{0,3})?))?\s*$/i.exec(part);
    if (match) values.set(match[1].toLowerCase(), Number(match[2] ?? 1));
  }
  return {
    br: values.get('br') ?? values.get('*') ?? 0,
    identity: values.get('identity') ?? (values.get('*') === 0 ? 0 : 1),
  };
}

function matchesEtag(header, etag) {
  const opaque = etag.replace(/^W\//, '');
  return header?.split(',').some(value => {
    const tag = value.trim();
    return tag === '*' || tag.replace(/^W\//, '') === opaque;
  });
}

export function createStaticServer({
  root = resolve(projectRoot, 'web'),
  compressionRoot = resolve(projectRoot, '.cache/http-compression'),
} = {}) {
  root = resolve(root);
  compressionRoot = resolve(compressionRoot);
  let files = {};
  try {
    const index = JSON.parse(readFileSync(resolve(compressionRoot, 'index.json'), 'utf8'));
    if (index.version === 1 && index.files && typeof index.files === 'object') files = index.files;
  } catch { /* The compression cache is optional. */ }
  const verified = new Map();

  async function verify(path, file, expectedHash, handle) {
    const key = fingerprint(file);
    const cached = verified.get(path);
    if (cached?.key === key && cached.hash === expectedHash) return cached.result;
    const result = (async () => {
      const hash = createHash('sha256');
      const stream = handle ? handle.createReadStream({ start: 0, autoClose: false }) : createReadStream(path);
      for await (const bytes of stream) hash.update(bytes);
      const after = handle ? await handle.stat() : await stat(path);
      return fingerprint(after) === key && hash.digest('hex') === expectedHash;
    })().catch(() => false);
    verified.set(path, { key, hash: expectedHash, result });
    return result;
  }

  async function brotli(pathname, path, source) {
    const entry = Object.hasOwn(files, pathname) ? files[pathname] : undefined;
    if (!entry?.source || !entry.brotli || !sameSource(source, entry.source)
      || !hashPattern.test(entry.source.sha256) || !hashPattern.test(entry.brotli.sha256)
      || entry.brotli.file !== `${entry.brotli.sha256}.br`
      || !Number.isSafeInteger(entry.brotli.size) || entry.brotli.size < 0) return;
    const compressedPath = resolve(compressionRoot, entry.brotli.file);
    let handle;
    try {
      const actualPath = await realpath(compressedPath);
      const actualRoot = await realpath(compressionRoot);
      if (!actualPath.startsWith(actualRoot + sep)) return;
      handle = await open(actualPath, 'r');
      const compressed = await handle.stat();
      if (!compressed.isFile() || compressed.size !== entry.brotli.size
        || !await verify(path, source, entry.source.sha256)
        || !await verify(actualPath, compressed, entry.brotli.sha256, handle)
        || !sameSource(await stat(path), source)
        || fingerprint(await handle.stat()) !== fingerprint(compressed)) return;
      const selected = { handle, size: compressed.size, etag: `"br-${entry.brotli.sha256}"` };
      handle = undefined;
      return selected;
    } catch { /* Invalid variants must not prevent identity responses. */ }
    finally { await handle?.close(); }
  }

  return createServer(async (request, response) => {
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      response.writeHead(405, { Allow: 'GET, HEAD' }).end();
      return;
    }
    let representation;
    try {
      const pathname = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
      const path = resolve(root, `.${pathname === '/' ? '/index.html' : pathname}`);
      if (!path.startsWith(root + sep)) {
        response.writeHead(403).end('Forbidden');
        return;
      }
      let file = await stat(path);
      if (!file.isFile()) {
        response.writeHead(404).end('Not found');
        return;
      }
      const qualities = encodingQualities(request.headers['accept-encoding']);
      const canCompress = eligible(path);
      const headers = {
        'Cache-Control': pathname.startsWith('/assets/runtime/') ? 'public, max-age=31536000, immutable' : 'no-cache',
        ...(canCompress ? { Vary: 'Accept-Encoding' } : {}),
      };
      if (canCompress && qualities.br > 0 && qualities.br >= qualities.identity) {
        representation = await brotli(pathname, path, file);
        if (!representation) {
          file = await stat(path);
          if (!file.isFile()) {
            response.writeHead(404).end('Not found');
            return;
          }
        }
      }
      if (!representation && qualities.identity === 0) {
        response.writeHead(406, headers).end();
        return;
      }
      headers.ETag = representation?.etag ?? `W/"identity-${file.size.toString(16)}-${file.mtimeMs.toString(16)}-${file.ctimeMs.toString(16)}"`;
      if (representation) headers['Content-Encoding'] = 'br';
      if (matchesEtag(request.headers['if-none-match'], headers.ETag)) {
        response.writeHead(304, headers).end();
        return;
      }
      response.writeHead(200, {
        ...headers,
        'Content-Type': types.get(extname(path)) ?? 'application/octet-stream',
        'Content-Length': representation?.size ?? file.size,
        'Last-Modified': file.mtime.toUTCString(),
      });
      if (request.method === 'HEAD') {
        response.end();
        return;
      }
      const stream = representation ? representation.handle.createReadStream({ start: 0 }) : createReadStream(path);
      representation = undefined;
      response.on('close', () => stream.destroy());
      stream.on('error', () => response.destroy()).pipe(response);
    } catch (error) {
      const code = error.code === 'ENOENT' || error.code === 'ENOTDIR' ? 404 : 400;
      if (response.headersSent) response.destroy();
      else response.writeHead(code).end(code === 404 ? 'Not found' : 'Invalid request');
    } finally { await representation?.handle.close(); }
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const port = Number(process.env.PORT ?? 4173);
  createStaticServer().listen(port, '127.0.0.1', function () {
    console.log(`The Long Slop: http://127.0.0.1:${this.address().port}`);
  });
}
