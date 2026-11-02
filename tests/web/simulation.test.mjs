import test from 'node:test';
import assert from 'node:assert/strict';
import {createSimulation} from '../../web/simulation.js';

function fakeWorker(){
 const listeners={},posted=[];
 return {posted,addEventListener:(type,fn)=>{listeners[type]=fn;},postMessage:(data,transfer)=>posted.push({data,transfer}),
  reply:data=>listeners.message({data}),fail:message=>listeners.error({message})};
}

test('start moves the manifest and resolves with the startup bytes',async()=>{
 const worker=fakeWorker(),simulation=createSimulation(worker,()=>assert.fail());
 const manifest=new Uint8Array([1,2]),save=new Uint8Array([3]),started=simulation.start(manifest,'hash',2500,1,-1,7,2,save);
 const {data,transfer}=worker.posted[0];
 assert.deepEqual({...data.start,manifest:undefined,save:undefined},{manifest:undefined,hash:'hash',drawDistance:2500,biome:1,startCar:-1,seed:7,options:2,save:undefined});
 assert.deepEqual(transfer,[manifest.buffer,save.buffer]);
 const startup=new Uint8Array([9]);
 worker.reply({startup,loadError:'bad save'});
 assert.deepEqual(await started,{startup,loadError:'bad save'});
});

test('a command is copied, its frame is read once, and stats reset',()=>{
 const worker=fakeWorker(),simulation=createSimulation(worker,()=>assert.fail()),memory=new Uint8Array([5,6,7]);
 simulation.send(memory.subarray(1));
 memory[1]=0;
 const {data,transfer}=worker.posted[0];
 assert.deepEqual([...data.command],[6,7]);
 assert.equal(transfer[0],data.command.buffer);
 assert.equal(simulation.frameLength(),-1);
 worker.reply({frame:new Uint8Array([3,4]),tickMs:2,wasmBytes:64});
 worker.reply({frame:new Uint8Array([8,9]),tickMs:4,wasmBytes:96});
 assert.equal(simulation.frameLength(),2);
 const out=new Uint8Array(2);
 simulation.read(out);
 assert.deepEqual([...out],[8,9]);
 assert.equal(simulation.frameLength(),-1);
 assert.deepEqual(simulation.stats(),{tickMs:3,wasmBytes:96});
 assert.deepEqual(simulation.stats(),{tickMs:null,wasmBytes:96});
});

test('a worker failure fails once, rejects the start and drops later frames',async()=>{
 const worker=fakeWorker(),failures=[],simulation=createSimulation(worker,message=>failures.push(message));
 const started=simulation.start(new Uint8Array(1),'',0,0,-1,0,0,new Uint8Array());
 worker.reply({error:'boom'});
 worker.fail('stopped');
 await assert.rejects(started,/boom/);
 assert.deepEqual(failures,['boom']);
 worker.reply({frame:new Uint8Array(1),tickMs:1,wasmBytes:1});
 assert.equal(simulation.frameLength(),-1);
 simulation.send(new Uint8Array(1));
 assert.equal(worker.posted.length,1);
});

test('the frame reports input use to the profile hooks',()=>{
 const worker=fakeWorker(),simulation=createSimulation(worker,()=>assert.fail()),calls=[];
 simulation.hooks({inputConsumed:()=>calls.push('consumed'),clearInput:()=>calls.push('clear')});
 worker.reply({frame:new Uint8Array(1),tickMs:1,wasmBytes:1,inputConsumed:true,clearInput:true});
 assert.deepEqual(calls,['clear','consumed']);
});

test('a net session before the start waits for it',async()=>{
 const worker=fakeWorker(),simulation=createSimulation(worker,()=>assert.fail());
 const started=simulation.start(new Uint8Array(1),'',0,0,-1,0,0,new Uint8Array());
 simulation.netSession(2,1,()=>{});simulation.netPacket(new Uint8Array([1]));simulation.netLeave(3);
 assert.equal(worker.posted.length,1);
 worker.reply({startup:new Uint8Array([9])});await started;
 assert.deepEqual(worker.posted[1].data,{netSession:{role:2,slot:1}});
});
