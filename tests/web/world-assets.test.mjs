import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {createServer} from 'node:http';
import {brotliCompressSync} from 'node:zlib';
import {configureAssetCatalog, readAsset} from '../../web/world-assets.js';

const base = 'assets/runtime/test/';
const blocks = Uint8Array.from({length: 8192}, (_, i) => i % 251);
const receipt = {file: 'reference/texture.bc', bytes: blocks.length,
  sha256: createHash('sha256').update(blocks).digest('hex'), image_size: null};

test('real fetch checks decoded Brotli bytes against the receipt', {timeout: 5000}, async t => {
  let body = blocks;
  let encoding = 'br';
  let declared;
  let holdBody = false;
  const server = createServer((_request, response) => {
    const wire = encoding === 'br' ? brotliCompressSync(body) : body;
    response.writeHead(200, {
      'Content-Type': 'application/octet-stream',
      'Content-Encoding': encoding,
      'Content-Length': declared ?? wire.length,
    });
    if (holdBody) response.flushHeaders();
    else response.end(wire);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}/`;
  const fetch = globalThis.fetch;
  const fetched = [];
  configureAssetCatalog(base, {files: [receipt], plans: [], dependency_staging_reserve: 1}, value => fetched.push(value));
  globalThis.fetch = (url, options) => fetch(new URL(url, origin), options);
  try {
    await t.test('fetch decodes the body and keeps the encoded Content-Length', async () => {
      const response = await fetch(new URL(base + receipt.file, origin));
      assert.equal(response.headers.get('content-encoding'), 'br');
      assert.equal(Number(response.headers.get('content-length')), brotliCompressSync(blocks).length);
      assert.notEqual(Number(response.headers.get('content-length')), receipt.bytes);
      assert.deepEqual(new Uint8Array(await response.arrayBuffer()), blocks);
    });
    await t.test('valid decoded body passes with decoded byte diagnostics', async () => {
      assert.deepEqual(await readAsset(base, receipt), blocks);
      assert.equal(fetched.at(-1).receivedBytes, blocks.length);
      assert.equal(fetched.at(-1).requiredBytes, blocks.length);
    });
    for (const [name, bytes, error] of [
      ['short', blocks.slice(0, -1), /Asset length differs/],
      ['trailing', Uint8Array.from([...blocks, 0]), /Asset length exceeds receipt/],
      ['corrupt', new Uint8Array(blocks.length), /SHA-256 mismatch/],
    ]) {
      await t.test(`${name} decoded body fails`, async () => {
        body = bytes;
        await assert.rejects(readAsset(base, receipt), error);
        assert.equal(fetched.at(-1).receivedBytes, bytes.length);
      });
    }
    await t.test('abort rejects a pending encoded body read', async () => {
      body = blocks;
      holdBody = true;
      const controller = new AbortController();
      globalThis.fetch = async (url, options) => {
        const response = await fetch(new URL(url, origin), options);
        setImmediate(() => controller.abort());
        return response;
      };
      try {
        await assert.rejects(readAsset(base, receipt, controller.signal), {name: 'AbortError'});
        assert.equal(fetched.at(-1).receivedBytes, 0);
      } finally {
        holdBody = false;
        globalThis.fetch = (url, options) => fetch(new URL(url, origin), options);
      }
    });
    await t.test('identity declared length still must match the receipt', async () => {
      body = blocks;
      encoding = 'identity';
      declared = blocks.length + 1;
      await assert.rejects(readAsset(base, receipt), /Asset declared size differs/);
      assert.equal(fetched.at(-1).receivedBytes, 0);
    });
  } finally {
    globalThis.fetch = fetch;
    configureAssetCatalog(base, {files: [receipt], plans: [], dependency_staging_reserve: 1});
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});

test('decoded overflow cancels the reader and releases its lock', async () => {
  const fetch = globalThis.fetch;
  let cancelled = false;
  const response = new Response(new ReadableStream({
    start(controller) { controller.enqueue(new Uint8Array(blocks.length + 1)); },
    cancel() { cancelled = true; },
  }), {headers: {'Content-Encoding': 'br', 'Content-Length': '1'}});
  globalThis.fetch = async () => response;
  try {
    await assert.rejects(readAsset(base, receipt), /Asset length exceeds receipt/);
    assert.equal(cancelled, true);
    assert.equal(response.body.locked, false);
  } finally {
    globalThis.fetch = fetch;
  }
});

test('invalid declared length cancels an encoded body before reading', async () => {
  const fetch = globalThis.fetch;
  try {
    for (const declared of ['-1', '1.5', '1e3', '']) {
      let cancelled = false;
      globalThis.fetch = async () => new Response(new ReadableStream({
        cancel() { cancelled = true; },
      }), {headers: {'Content-Encoding': 'br', 'Content-Length': declared}});
      await assert.rejects(readAsset(base, receipt), /Asset declared size differs/);
      assert.equal(cancelled, true);
    }
  } finally {
    globalThis.fetch = fetch;
  }
});
