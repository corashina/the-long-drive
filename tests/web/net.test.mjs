import test from 'node:test';
import assert from 'node:assert/strict';
import {createNet,ROLE,STATUS,PREFIX,TIMEOUT_MS,TICK_MS,RELIABLE,DATAGRAM,datagrams} from '../../web/net.js';

// Packets with a u16 length each. `reliable` sets bit `RELIABLE` of each length.
function joinPackets(packets,reliable=false){
 const out=new Uint8Array(packets.reduce((n,p)=>n+2+p.length,0));
 let at=0;
 for(const p of packets){out[at]=p.length&255;out[at+1]=p.length>>8|(reliable?RELIABLE>>8:0);out.set(p,at+2);at+=2+p.length;}
 return out;
}
function unbatch(batch){
 const packets=[];
 for(let at=0;at+2<=batch.length;){const n=batch[at]|batch[at+1]<<8;packets.push([...batch.subarray(at+2,at+2+n)]);at+=2+n;}
 return packets;
}

// A PeerJS stand-in. Peers meet in one in-memory broker. Messages pass at once.
function broker(){
 const peers=new Map(),muted=new Set(),sent={};let next=0;
 const handlers=()=>{const h={};return {h,on:(e,fn)=>{(h[e]??=[]).push(fn);},fire:(e,...a)=>(h[e]??[]).forEach(fn=>fn(...a))};};
 function conn(local,remote,label){
  const a=handlers(),b=handlers();
  const ends=[{...a,peer:remote,label,open:true,send:m=>{sent[label]=(sent[label]??0)+1;if(!muted.has(local))b.fire('data',m);},close(){if(this.open){this.open=false;b.fire('close');}}},
   {...b,peer:local,label,open:true,send:m=>{sent[label]=(sent[label]??0)+1;if(!muted.has(remote))a.fire('data',m);},close(){if(this.open){this.open=false;a.fire('close');}}}];
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
  // A media call. The callee answers, then the caller stream arrives at the callee.
  call(id,stream,{metadata}={}){
   const target=peers.get(id),a=handlers(),b=handlers();
   const mine={...a,peer:id,metadata,close(){if(this.open){this.open=false;mine.fire('close');theirs.fire('close');}},open:true};
   const theirs={...b,peer:this.id,metadata,open:true,close:()=>mine.close(),answer(){queueMicrotask(()=>{if(mine.open)theirs.fire('stream',stream);});}};
   if(target)queueMicrotask(()=>target.fire('call',theirs));
   return mine;
  }
  destroy(){peers.delete(this.id);}
 }
 return {Peer,peers,muted,sent};
}
const tick=()=>new Promise(r=>setTimeout(r,0));
function setup(Peer,clock,extra={}){
 const events={packets:[],leaves:[],status:[],chat:[],saves:[],sessions:[],looks:[],voices:[]};let timer=null;
 const net=createNet({Peer,buildId:'b1',exportHash:'h1',capture:async()=>'{"world":1}',now:()=>clock.t,
  setTimer:fn=>{timer=fn;return 1;},clearTimer:()=>{timer=null;},...extra,
  on:{packets:b=>events.packets.push(...unbatch(b)),leave:s=>events.leaves.push(s),status:s=>events.status.push(s),chat:c=>events.chat.push(c),
   save:b=>events.saves.push(new TextDecoder().decode(b)),session:(r,s)=>events.sessions.push([r,s]),looks:l=>events.looks.push(l),voice:(slot,stream)=>events.voices.push([slot,stream])}});
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

test('a frame of packets goes as one message per connection, and the host forwards it as one',async()=>{
 const {Peer,sent}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock),b=setup(Peer,clock);
 const code=await host.net.host({name:'H'});
 a.net.join({code,name:'A'});b.net.join({code,name:'B'});
 for(let i=0;i<10;i++)await tick();
 const before={...sent};
 const fast=[[10,5,1],[11,5,2,3]].map(p=>new Uint8Array(p)),sure=[[29,5,4]].map(p=>new Uint8Array(p));
 const outbox=new Uint8Array([...joinPackets(fast),...joinPackets(sure,true),...joinPackets(fast.slice(0,1))]);
 a.net.send(outbox);
 // A sends 1 pose and 1 reliable message. The host forwards each as 1 message to B.
 assert.equal(sent.pose-(before.pose??0),2);
 assert.equal(sent.reliable-(before.reliable??0),2);
 assert.deepEqual(b.events.packets,[[10,1,1],[11,1,2,3],[10,1,1],[29,1,4]]);
 assert.deepEqual(host.events.packets,b.events.packets);
});

test('a large batch splits at packet ends into datagrams',()=>{
 const big=new Uint8Array(600),parts=[];
 datagrams(joinPackets([big,big,big,new Uint8Array(2000)]),part=>parts.push(part.length));
 assert.deepEqual(parts,[602,602,602,2002]);
 assert.ok(parts.slice(0,3).every(n=>n<=DATAGRAM));
});

test('reliable packets ride the reliable connection and the host forwards them',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock),b=setup(Peer,clock);
 const code=await host.net.host({name:'H'});
 a.net.join({code,name:'A'});b.net.join({code,name:'B'});
 for(let i=0;i<10;i++)await tick();
 const reliable=bytes=>joinPackets([new Uint8Array(bytes)],true);
 a.net.send(reliable([29,5,1]));
 assert.deepEqual(host.events.packets,[[29,1,1]]);
 assert.deepEqual(b.events.packets,[[29,1,1]]);
 host.net.send(reliable([28,0,3]));
 assert.deepEqual(a.events.packets,[[28,0,3]]);
 assert.deepEqual(b.events.packets,[[29,1,1],[28,0,3]]);
});

test('a silent guest times out and the others see it leave',async()=>{
 const {Peer,peers,muted}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock),b=setup(Peer,clock);
 const code=await host.net.host({});
 a.net.join({code,name:'A'});
 for(let i=0;i<10;i++)await tick();
 b.net.join({code,name:'B'});
 for(let i=0;i<10;i++)await tick();
 a.net.ready();b.net.ready();
 // Guest A goes silent. Its first peer id sends nothing more.
 muted.add([...peers.keys()].find(id=>!id.startsWith(PREFIX)));
 while(clock.t<=TIMEOUT_MS){clock.t+=TICK_MS/2;b.run();host.run();}
 assert.deepEqual(host.events.leaves,[1]);
 assert.deepEqual(b.events.leaves,[1]);
});

test('a guest that loads the host world with a stalled page stays in the room',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock);
 const code=await host.net.host({});
 a.net.join({code,name:'A'});
 for(let i=0;i<10;i++)await tick();
 // The guest page sends nothing for 3 timeouts while it loads. Then its stalled tick runs.
 while(clock.t<=3*TIMEOUT_MS){clock.t+=TICK_MS;host.run();}
 a.run();a.net.ready();
 for(let i=0;i<3;i++){clock.t+=TICK_MS;host.run();a.run();}
 assert.deepEqual(host.events.leaves,[]);
 assert.equal(a.net.role,ROLE.GUEST);
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

test('guests see the host leave, and the room code clears',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock);
 const code=await host.net.host({});
 a.net.join({code});
 for(let i=0;i<10;i++)await tick();
 assert.equal(host.net.code,code);
 assert.equal(a.net.code,code);
 host.net.leave();
 assert.deepEqual(a.events.status.at(-1),STATUS.hostLeft);
 assert.equal(a.net.role,ROLE.OFF);
 assert.equal(host.net.code,'');
 assert.equal(a.net.code,'');
 assert.ok(PREFIX);
});

// Roadmap MP5: an outfit change on one peer reaches every other peer through the host.
test('tp_mp5_an_outfit_change_reaches_the_other_peers',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock),b=setup(Peer,clock);
 const code=await host.net.host({name:'H',outfit:{model:0}});
 a.net.join({code,name:'A',outfit:{model:1}});b.net.join({code,name:'B',outfit:{model:2}});
 for(let i=0;i<10;i++)await tick();
 const last=peer=>peer.events.looks.at(-1);
 assert.deepEqual(last(b).get(1),{name:'A',outfit:{model:1}});
 a.net.look({model:3});
 for(const peer of [host,b,a])assert.deepEqual(last(peer).get(1),{name:'A',outfit:{model:3}});
 host.net.look({model:4});
 for(const peer of [a,b])assert.deepEqual(last(peer).get(0),{name:'H',outfit:{model:4}});
 // The other figures keep their own outfits.
 assert.deepEqual(last(a).get(2),{name:'B',outfit:{model:2}});
});

test('voice: each peer hears the others through the host, and a leave ends that voice',async()=>{
 const {Peer}=broker(),clock={t:0};
 const host=setup(Peer,clock),a=setup(Peer,clock),b=setup(Peer,clock);
 const code=await host.net.host({name:'H'});
 host.net.voice('mic-h');
 a.net.voice('mic-a');
 a.net.join({code,name:'A'});
 for(let i=0;i<10;i++)await tick();
 b.net.voice('mic-b');
 b.net.join({code,name:'B'});
 for(let i=0;i<20;i++)await tick();
 const heard=e=>Object.fromEntries(e.voices.filter(([,s])=>s));
 assert.deepEqual(heard(host.events),{1:'mic-a',2:'mic-b'});
 assert.deepEqual(heard(a.events),{0:'mic-h',2:'mic-b'});
 assert.deepEqual(heard(b.events),{0:'mic-h',1:'mic-a'});
 a.net.leave();
 for(let i=0;i<10;i++)await tick();
 assert.deepEqual(b.events.voices.at(-1),[1,null]);
 assert.deepEqual(host.events.voices.at(-1),[1,null]);
});
