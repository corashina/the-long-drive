import test from 'node:test';
import assert from 'node:assert/strict';
import {createTerrain} from '../../web/terrain.js';

function fakeWorker(){
 const listeners={},posted=[];
 return {posted,terminated:false,addEventListener:(type,fn)=>{listeners[type]=fn;},postMessage:(data,transfer)=>posted.push({data,transfer}),terminate(){this.terminated=true;},
  reply:data=>listeners.message({data})};
}

test('a terrain job completes once and a fatal reply stops the transport',()=>{
 const worker=fakeWorker(),terrain=createTerrain(worker,null);
 worker.reply({type:'initialized',wasmBytes:1024});
 const id=terrain.submit(new Uint8Array([1,2,3]));
 assert.equal(worker.posted[0].data.type,'evaluate');
 assert.equal(terrain.poll(),-2);
 const rows=()=>new Float32Array(3);
 worker.reply({type:'result',id,vertices:rows(),normals:rows(),tangents:new Float32Array(4),uv:new Float32Array(2),colors:new Float32Array(4),triangles:new Uint32Array(3),wasmBytes:1024});
 assert.equal(terrain.poll(),id);
 assert.equal(terrain.vertexCount(),1);
 terrain.discard();
 assert.equal(terrain.poll(),-1);
 worker.reply({type:'fatal',error:'out of memory'});
 assert.equal(worker.terminated,true);
 assert.throws(()=>terrain.poll(),/out of memory/);
});

