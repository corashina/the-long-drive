import test from 'node:test';
import assert from 'node:assert/strict';
import {createNet,ROLE,STATUS,PREFIX,TIMEOUT_MS} from '../../web/net.js';
import {joinPackets} from '../../web/simulation.js';

// A PeerJS stand-in: peers meet in one in-memory broker. Messages pass at once.
function broker(){
 const peers=new Map(),muted=new Set();let next=0;
 const handlers=()=>{const h={};return {h,on:(e,fn)=>{(h[e]??=[]).push(fn);},fire:(e,...a)=>(h[e]??[]).forEach(fn=>fn(...a))};};
 function conn(local,remote,label){
  const a=handlers(),b=handlers();
  const ends=[{...a,peer:remote,label,open:true,send:m=>{if(!muted.has(local))b.fire('data',m);},close(){if(this.open){this.open=false;b.fire('close');}}},
   {...b,peer:local,label,open:true,send:m=>{if(!muted.has(remote))a.fire('data',m);},close(){if(this.open){this.open=false;a.fire('close');}}}];
  return ends;
 }
 class Peer{
  constructor(id=`p${next++}`){
   this.id=id;Object.assign(this,handlers());
   if(peers.has(id))queueMicrotask(()=>this.fire('error',{type:'unavailable-id'}));
   else{peers.set(id,this);queueMicrotask(()=>this.fire('open',id));}
  }
  connect(id,{label}){
   const target=peers.get(id),[mine,theirs]=conn(this.id,id,label);
   if(!target){queueMicrotask(()=>this.fire('error',{type:'peer-unavailable'}));return mine;}
   queueMicrotask(()=>{target.fire('connection',theirs);mine.fire('open');});
   return mine;
  }
  destroy(){peers.delete(this.id);}
 }
 return {Peer,peers,muted};
}
const tick=()=>new Promise(r=>setTimeout(r,0));
function setup(Peer,clock,extra={}){
 const events={packets:[],leaves:[],status:[],chat:[],saves:[],sessions:[]};let timer=null;
 const net=createNet({Peer,buildId:'b1',exportHash:'h1',capture:async()=>'{"world":1}',now:()=>clock.t,
  setTimer:fn=>{timer=fn;return 1;},clearTimer:()=>{timer=null;},...extra,
  on:{packet:p=>events.packets.push([...p]),leave:s=>events.leaves.push(s),status:s=>events.status.push(s),chat:c=>events.chat.push(c),
   save:b=>events.saves.push(new TextDecoder().decode(b)),session:(r,s)=>events.sessions.push([r,s])}});
 return {net,events,run:()=>timer?.()};
}

test('the host forwards guest packets to the other guests and not back',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock),b=setup(Peer,clock);
 const code=await host.net.host({name:'H'});
 a.net.join({code,name:'A'});b.net.join({code,name:'B'});
 for(let i=0;i<10;i++)await tick();
 assert.deepEqual(a.events.saves,['{"world":1}']);
 assert.deepEqual(a.events.sessions,[[ROLE.GUEST,1]]);
 assert.deepEqual(b.events.sessions,[[ROLE.GUEST,2]]);
 // Guest A claims slot 5. The host stamps its real slot.
 a.net.send(joinPackets([new Uint8Array([10,5,7])]));
 assert.deepEqual(host.events.packets,[[10,1,7]]);
 assert.deepEqual(b.events.packets,[[10,1,7]]);
 assert.deepEqual(a.events.packets,[]);
 a.net.chat('hi');
 assert.deepEqual(b.events.chat,[{name:'A',text:'hi'}]);
 assert.deepEqual(host.events.chat,[{name:'A',text:'hi'}]);
});

test('a silent guest times out and the others see it leave',async()=>{
 const {Peer,peers,muted}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock),b=setup(Peer,clock);
 const code=await host.net.host({});
 a.net.join({code,name:'A'});
 for(let i=0;i<10;i++)await tick();
 b.net.join({code,name:'B'});
 for(let i=0;i<10;i++)await tick();
 // Guest A goes silent: its first peer id sends nothing more.
 muted.add([...peers.keys()].find(id=>!id.startsWith(PREFIX)));
 clock.t=TIMEOUT_MS-1;b.run();host.run();
 clock.t=TIMEOUT_MS+1;b.run();host.run();
 assert.deepEqual(host.events.leaves,[1]);
 assert.deepEqual(b.events.leaves,[1]);
});

test('refuse reasons reach the join status',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock);
 const code=await host.net.host({maxPlayers:2,password:'pw'});
 const wrong=setup(Peer,clock);wrong.net.join({code,name:'W',password:'no'});
 const old=setup(Peer,clock,{buildId:'b0'});old.net.join({code,password:'pw'});
 const first=setup(Peer,clock);first.net.join({code,password:'pw'});
 for(let i=0;i<10;i++)await tick();
 const full=setup(Peer,clock);full.net.join({code,password:'pw'});
 for(let i=0;i<10;i++)await tick();
 const missing=setup(Peer,clock);missing.net.join({code:'ZZZZZZ'});
 for(let i=0;i<10;i++)await tick();
 assert.deepEqual(wrong.events.status,[STATUS.password]);
 assert.deepEqual(old.events.status,[STATUS.version]);
 assert.deepEqual(full.events.status,[STATUS.full]);
 assert.deepEqual(missing.events.status,[STATUS.notFound]);
 assert.equal(first.net.role,ROLE.GUEST);
});

test('guests see the host leave',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock);
 const code=await host.net.host({});
 a.net.join({code});
 for(let i=0;i<10;i++)await tick();
 host.net.leave();
 assert.deepEqual(a.events.status.at(-1),STATUS.hostLeft);
 assert.equal(a.net.role,ROLE.OFF);
 assert.ok(PREFIX);
});
