import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {GRASS_DATASET_HEADER,GRASS_PATCH_BYTES} from '../../web/grass.js';

test('grass dataset layout matches the Rust writer',()=>{
 const codec=readFileSync(new URL('../../crates/grass-mesh/src/codec.rs',import.meta.url),'utf8');
 const length=codec.match(/fn dataset_encoded_len[\s\S]*?checked_mul\((\d+)\)[\s\S]*?checked_add\((\d+)\)/);
 assert.ok(length,'dataset_encoded_len shape changed');
 assert.equal(GRASS_PATCH_BYTES,Number(length[1]));
 assert.equal(GRASS_DATASET_HEADER,Number(length[2]));
});
