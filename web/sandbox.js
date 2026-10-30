import {parseSeed} from './game-mode.js';
// Test sandbox of `?sandbox`. Claude runs the ROADMAP test pass with it in the browser.
// URL flags `date=MM-DD` and `seed=N` apply at page load. Each call resolves with its reply.
// New games use seed 42 unless `seed` is set, so test coordinates find the same world.
export function readSandbox(search){
 const params=new URLSearchParams(search);if(!params.has('sandbox'))return null;
 const date=/^(\d\d)-(\d\d)$/.exec(params.get('date')??'');
 return {calendar:date?Number(date[1])*100+Number(date[2]):null,seed:parseSeed(params.get('seed')??'42')};
}
// Source `fluidenum` indices. Other fluids go by number.
const FLUIDS={petrol:0,diesel:5};
export function installSandbox(target,enabled){
 let next=1,queue=[];const waiting=new Map();
 target.gameSandboxTake=()=>{if(!queue.length)return undefined;const json=JSON.stringify(queue);queue=[];return json;};
 target.gameSandboxReply=json=>{const reply=JSON.parse(json),resolve=waiting.get(reply.id);if(!resolve){console.warn('Sandbox:',reply.message);return;}waiting.delete(reply.id);resolve(reply);};
 if(!enabled)return;
 const send=(action,fields)=>new Promise(resolve=>{const id=next++;waiting.set(id,resolve);queue.push({id,action,...fields});});
 target.sandbox={
  teleport:(x,z)=>send('teleport',{x,z}),garage:()=>send('garage',{}),building:name=>send('building',{name}),
  spawn:(name,forward=3)=>send('spawn',{name,forward}),hand:name=>send('hand',{name}),
  fill:(fluid,liters)=>send('fill',{fluid:FLUIDS[fluid]??fluid,liters}),cycle:seconds=>send('cycle',{seconds}),state:(radius=50)=>send('state',{radius}),
 };
}
