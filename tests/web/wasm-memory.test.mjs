import test from 'node:test';
import assert from 'node:assert/strict';
import {wasmMemoryFailure,wasmMemoryLimit} from '../../web/wasm-memory.js';

const MIB=1048576;
test('the WASM memory limit is half the device memory, else 4 GiB', () => {
 assert.equal(wasmMemoryLimit(8),4096*MIB);
 assert.equal(wasmMemoryLimit(2),1024*MIB);
 assert.equal(wasmMemoryLimit(undefined),4096*MIB);
});
test('only a total above the limit stops the game', () => {
 assert.equal(wasmMemoryFailure(1024*MIB,1024*MIB),null);
 assert.equal(wasmMemoryFailure(1025*MIB,1024*MIB),'WASM memory 1025 MiB is above the 1024 MiB limit of this device. Lower the draw distance.');
});
