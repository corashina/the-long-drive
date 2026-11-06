// Multiplayer peers of source `sns` and `snl`. PeerJS runs on the page thread, because Chrome has no WebRTC in workers.
// The peers form a star like the source. Guests talk only to the host. The host forwards guest messages to the other guests.
// Each guest has a `reliable` connection for the session messages and the save, and a `pose` connection for
// the simulation packets. A packet is the type byte, the sender slot byte, then a body that only Rust reads.
// A batch is packets with a u16 length each, like the Rust inbox. One frame sends one batch per connection,
// so the message count does not grow with the packet count.
// Bit `RELIABLE` of an outbound packet length sends the packet on `reliable` as `{t:'sim',data}`, like source `EP2PSendReliable`.
// The save goes gzipped, because the JSON is large and compresses well.
// A guest loads the host world for many seconds with a busy page thread, so silence until `ready` uses `LOAD_TIMEOUT_MS`.
export const RELIABLE=0x8000;
// The largest `pose` message. A larger message splits into IP fragments, and the loss of one drops all of it.
export const DATAGRAM=1150;
export const PREFIX='tls-',MAX_PLAYERS=8,TIMEOUT_MS=5000,JOIN_TIMEOUT_MS=15000,LOAD_TIMEOUT_MS=120000,TICK_MS=1000,CHUNK=16384,CHAT_BYTES=200;
const LETTERS='ABCDEFGHJKLMNPQRSTUVWXYZ';
export const ROLE={OFF:0,HOST:1,GUEST:2};
export const STATUS={
 notFound:'Room not found.',full:'Room is full.',password:'Wrong password.',version:'Different game version.',
 server:'Cannot reach the connection server.',connect:'Cannot connect to the host.',hostLeft:'Host left.',save:'The host world could not be loaded.',
};
export function roomCode(random=Math.random){return Array.from({length:6},()=>LETTERS[Math.floor(random()*LETTERS.length)]).join('');}
export function validCode(code){return typeof code==='string'&&code.length===6&&[...code].every(c=>LETTERS.includes(c));}
const bytes=data=>data instanceof Uint8Array?data:new Uint8Array(data);
const transform=(data,stream)=>new Response(new Blob([data]).stream().pipeThrough(stream)).arrayBuffer().then(b=>new Uint8Array(b));
export const gzip=data=>transform(data,new CompressionStream('gzip'));
export const gunzip=data=>transform(data,new DecompressionStream('gzip'));
// Splits a frame outbox into the unreliable and the reliable batch. Each length loses bit `RELIABLE`.
export function splitOutbox(outbox){
 let sizes=[0,0];
 for(let at=0;at+2<=outbox.length;){const raw=outbox[at]|outbox[at+1]<<8,n=raw&~RELIABLE;if(at+2+n>outbox.length)break;sizes[raw&RELIABLE?1:0]+=2+n;at+=2+n;}
 const out=sizes.map(n=>new Uint8Array(n));sizes=[0,0];
 for(let at=0;at+2<=outbox.length;){
  const raw=outbox[at]|outbox[at+1]<<8,n=raw&~RELIABLE,k=raw&RELIABLE?1:0;if(at+2+n>outbox.length)break;
  out[k].set(outbox.subarray(at,at+2+n),sizes[k]);out[k][sizes[k]+1]=n>>8;sizes[k]+=2+n;at+=2+n;
 }
 return out;
}
// Calls `send` with parts of `batch` of up to `DATAGRAM` bytes, cut at packet ends. A larger packet goes alone.
export function datagrams(batch,send){
 let start=0;
 for(let at=0;at+2<=batch.length;){
  const end=at+2+(batch[at]|batch[at+1]<<8);
  if(end-start>DATAGRAM&&at>start){send(batch.subarray(start,at));start=at;}
  at=end;
 }
 if(start<batch.length)send(batch.subarray(start));
}
// A copy of a guest batch with sender `slot` in each packet. The host stamps it, so a guest cannot speak for another.
// A truncated tail drops. Null when no packet is left.
export function stamp(data,slot){
 const batch=bytes(data);let at=0;
 while(at+2<=batch.length){const end=at+2+(batch[at]|batch[at+1]<<8);if(end>batch.length)break;at=end;}
 if(at===0)return null;
 const out=batch.slice(0,at);
 for(let i=0;i<at;){const n=out[i]|out[i+1]<<8;if(n>=2)out[i+3]=slot;i+=2+n;}
 return out;
}
// Cuts a chat line to `CHAT_BYTES` UTF-8 bytes, like the source chat field limit.
export function chatText(text){
 const encoded=new TextEncoder().encode(String(text).trim());
 return new TextDecoder().decode(encoded.subarray(0,CHAT_BYTES)).replace(/�$/,'');
}

// `Peer`: the PeerJS class. `capture()`: the host save record as a string.
// Voice (source `SVoice`, host forwards). Each peer sends its microphone in a PeerJS media call. A guest calls the host.
// The host calls each guest once per other speaker, with that speaker's stream and `metadata.slot`.
// Events: `voice(slot,stream|null)`, `packet(bytes)`, `leave(slot)`, `looks(Map slot→{name,outfit})`, `chat({name,text})`, `status(text)`,
// `save(bytes)`, `download(chunks,total)` of the host world, `session(role,slot)`, `peers([{slot,name,ping}])`. `packets(batch)`: a batch of inbound packets.
export function createNet({Peer,buildId,exportHash,capture,on,now=()=>performance.now(),setTimer=setInterval,clearTimer=clearInterval}){
 let role=ROLE.OFF,slot=0,peer=null,timer=null,joinStarted=0,lastTick=0,me={name:'',outfit:null};
 let max=MAX_PLAYERS,password='',host=null,mic=null,micCall=null,room='';
 const guests=new Map(),looks=new Map();
 const emit=(name,...args)=>on[name]?.(...args);
 const peers=()=>emit('peers',[...looks].map(([s,l])=>({slot:s,name:l.name,ping:s===slot?null:(role===ROLE.HOST?guestBySlot(s)?.ping:host?.ping)??null})));
 const guestBySlot=s=>[...guests.values()].find(g=>g.slot===s);
 const setLook=(s,look)=>{if(look)looks.set(s,look);else looks.delete(s);emit('looks',new Map(looks));peers();};
 function stop(status){
  // Clear the role first. The closes below fire close events that must not stop again.
  const was=role;role=ROLE.OFF;slot=0;room='';looks.clear();
  if(timer!==null)clearTimer(timer);timer=null;
  const closing=[...guests.values()];guests.clear();
  for(const g of closing){g.reliable?.close();g.pose?.close();closeVoice(g);}
  const old=host;host=null;old?.reliable?.close();old?.pose?.close();
  micCall?.close();micCall=null;
  for(const call of old?.voices?.values()??[])call.close();
  peer?.destroy();peer=null;
  if(was!==ROLE.OFF)emit('session',ROLE.OFF,0);
  if(status)emit('status',status);
 }
 function failure(error){
  const type=error?.type;
  if(type==='peer-unavailable')stop(STATUS.notFound);
  else if(['unavailable-id','network','server-error','socket-error','browser-incompatible'].includes(type))stop(STATUS.server);
  else stop(error?.message??String(error));
 }
 // Host: send to every guest except `except`.
 function broadcast(message,except=-1){for(const g of guests.values())if(g.slot!==except&&g.welcomed)g.reliable.send(message);}
 function removeGuest(id){
  const g=guests.get(id);if(!g)return;
  guests.delete(id);g.reliable?.close();g.pose?.close();closeVoice(g);
  for(const other of guests.values())unrelay(other,g.slot);
  if(g.welcomed){emit('voice',g.slot,null);broadcast({t:'leave',slot:g.slot});emit('leave',g.slot);setLook(g.slot,null);}
 }
 async function hello(g,m){
  const refuse=reason=>{g.reliable.send({t:'refuse',reason});setTimeout(()=>removeGuest(g.id),500);};
  if(m.build!==buildId||m.exportHash!==exportHash)return refuse('version');
  if(password&&m.password!==password)return refuse('password');
  const used=new Set([0,...[...guests.values()].map(x=>x.slot)]);
  const free=Array.from({length:max-1},(_,i)=>i+1).find(s=>!used.has(s));
  if(free===undefined)return refuse('full');
  g.slot=free;
  let save;
  try{save=await gzip(new TextEncoder().encode(await capture()));}catch{return refuse('save');}
  if(!guests.has(g.id))return;
  const chunks=Math.ceil(save.length/CHUNK);
  g.reliable.send({t:'welcome',slot:free,chunks,looks:[...looks]});
  for(let i=0;i<chunks;i++)g.reliable.send({t:'save',i,data:save.subarray(i*CHUNK,(i+1)*CHUNK)});
  g.welcomed=true;
  if(mic)relay(g,0,mic);
  for(const other of guests.values())if(other!==g&&other.voiceIn)relay(g,other.slot,other.voiceIn);
  const look={name:chatText(m.name)||`Player ${free+1}`,outfit:m.outfit??null};
  setLook(free,look);broadcast({t:'look',slot:free,...look},free);
 }
 function hostMessage(g,m){
  g.seen=now();
  if(m.t==='hello'&&g.slot===undefined)return void hello(g,m);
  if(!g.welcomed)return;
  if(m.t==='look'){const look={name:chatText(m.name)||looks.get(g.slot)?.name,outfit:m.outfit??null};setLook(g.slot,look);broadcast({t:'look',slot:g.slot,...look},g.slot);}
  else if(m.t==='chat'){const text=chatText(m.text);if(!text)return;broadcast({t:'chat',slot:g.slot,text},g.slot);emit('chat',{name:looks.get(g.slot)?.name??'',text});}
  else if(m.t==='ping')g.reliable.send({t:'pong',at:m.at});
  else if(m.t==='pong'){g.ping=Math.round(now()-m.at);peers();}
  else if(m.t==='leave')removeGuest(g.id);
  else if(m.t==='ready')g.ready=true;
  else if(m.t==='sim')hostBatch(g,m.data,true);
 }
 // Host: a guest batch goes on as one message to each other guest.
 function hostBatch(g,data,reliable=false){
  g.seen=now();
  if(!g.welcomed)return;
  const batch=stamp(data,g.slot);
  if(!batch)return;
  const message=reliable?{t:'sim',data:batch}:null;
  for(const other of guests.values()){
   if(other===g||!other.welcomed)continue;
   if(reliable)other.reliable.send(message);else if(other.pose?.open)other.pose.send(batch);
  }
  emit('packets',batch);
 }
 function closeVoice(g){g.voiceCall?.close();g.voiceCall=null;g.voiceIn=null;for(const call of g.voiceOut.values())call.close();g.voiceOut.clear();}
 // Host: sends the voice of `from` to guest `g`.
 function relay(g,from,stream){g.voiceOut.get(from)?.close();g.voiceOut.set(from,peer.call(g.id,stream,{metadata:{slot:from}}));}
 // Host: stops sending the voice of `from` to guest `g`.
 function unrelay(g,from){g.voiceOut.get(from)?.close();g.voiceOut.delete(from);}
 // Host: a guest microphone. It plays here and goes to the other guests.
 function hostCall(call){
  const g=guests.get(call.peer);if(!g?.welcomed){call.close();return;}
  g.voiceCall?.close();g.voiceCall=call;call.answer();
  call.on('stream',stream=>{
   if(g.voiceCall!==call)return;
   g.voiceIn=stream;emit('voice',g.slot,stream);
   for(const other of guests.values())if(other!==g&&other.welcomed)relay(other,g.slot,stream);
  });
  call.on('close',()=>{
   if(g.voiceCall!==call)return;
   g.voiceCall=null;g.voiceIn=null;emit('voice',g.slot,null);
   for(const other of guests.values())unrelay(other,g.slot);
  });
 }
 // Guest: the voice of one speaker from the host.
 function guestCall(call){
  const from=call.metadata?.slot;
  if(!host?.welcomed||call.peer!==host.reliable.peer||!Number.isInteger(from)||from===slot){call.close();return;}
  host.voices.get(from)?.close();host.voices.set(from,call);call.answer();
  call.on('stream',stream=>{if(host?.voices.get(from)===call)emit('voice',from,stream);});
  call.on('close',()=>{if(host?.voices.get(from)!==call)return;host.voices.delete(from);emit('voice',from,null);});
 }
 function callHost(){micCall?.close();micCall=mic&&host?.welcomed?peer.call(host.reliable.peer,mic):null;}
 function accept(conn){
  const id=conn.peer;
  if(conn.label==='pose'){
   const g=guests.get(id);if(!g){conn.close();return;}
   g.pose=conn;conn.on('data',data=>hostBatch(g,data));return;
  }
  if(guests.has(id)){conn.close();return;}
  const g={id,reliable:conn,pose:null,slot:undefined,welcomed:false,ready:false,seen:now(),ping:null,voiceCall:null,voiceIn:null,voiceOut:new Map()};
  guests.set(id,g);
  conn.on('data',m=>hostMessage(g,m));
  conn.on('close',()=>removeGuest(id));
  conn.on('error',()=>removeGuest(id));
 }
 function tick(){
  const t=now(),stalled=t-lastTick>2*TICK_MS;lastTick=t;
  // A late tick means this page thread stalled. Messages can wait in the queue behind it, so silence is not proof.
  if(stalled){for(const g of guests.values())g.seen=t;if(host)host.seen=t;}
  if(role===ROLE.HOST){
   for(const g of [...guests.values()]){if(t-g.seen>(g.ready?TIMEOUT_MS:LOAD_TIMEOUT_MS))removeGuest(g.id);else if(g.welcomed)g.reliable.send({t:'ping',at:t});}
  }else if(role===ROLE.GUEST&&host){
   if(!host.welcomed){if(t-joinStarted>JOIN_TIMEOUT_MS)stop(STATUS.connect);return;}
   if(t-host.seen>(host.ready?TIMEOUT_MS:LOAD_TIMEOUT_MS))return stop(STATUS.hostLeft);
   host.reliable.send({t:'ping',at:t});
  }
 }
 function guestMessage(m){
  host.seen=now();
  if(m.t==='refuse')return stop(STATUS[m.reason]??STATUS.connect);
  if(m.t==='welcome'){
   slot=m.slot;host.welcomed=true;host.chunks=Array(m.chunks);host.received=0;
   for(const [s,look] of m.looks)looks.set(s,look);
   looks.set(slot,{name:me.name,outfit:me.outfit});emit('looks',new Map(looks));
   host.pose=peer.connect(host.reliable.peer,{label:'pose',reliable:false,serialization:'raw'});
   host.pose.on('data',data=>{host.seen=now();emit('packets',bytes(data));});
   callHost();
   if(m.chunks===0)emit('status',STATUS.save);else emit('download',0,m.chunks);
   return;
  }
  if(m.t==='save'&&host.chunks&&host.chunks[m.i]===undefined){
   host.chunks[m.i]=bytes(m.data);host.received++;
   emit('download',host.received,host.chunks.length);
   if(host.received===host.chunks.length){
    const save=new Uint8Array(host.chunks.reduce((n,c)=>n+c.length,0));let at=0;
    for(const c of host.chunks){save.set(c,at);at+=c.length;}
    host.chunks=null;
    const joined=host;
    gunzip(save).then(json=>{if(host!==joined)return;emit('session',ROLE.GUEST,slot);emit('save',json);peers();},()=>{if(host===joined)stop(STATUS.save);});
   }
   return;
  }
  if(m.t==='look')setLook(m.slot,{name:m.name,outfit:m.outfit});
  else if(m.t==='chat')emit('chat',{name:looks.get(m.slot)?.name??'',text:chatText(m.text)});
  else if(m.t==='leave'){host.voices.get(m.slot)?.close();host.voices.delete(m.slot);emit('voice',m.slot,null);emit('leave',m.slot);setLook(m.slot,null);}
  else if(m.t==='ping')host.reliable.send({t:'pong',at:m.at});
  else if(m.t==='pong'){host.ping=Math.round(now()-m.at);peers();}
  else if(m.t==='sim')emit('packets',bytes(m.data));
 }
 return {
  get role(){return role;},
  get slot(){return slot;},
  // The code of the open or joined room, or ''.
  get code(){return room;},
  // Opens a room. Resolves with the room code.
  host({maxPlayers=MAX_PLAYERS,password:pass='',name,outfit}){
   stop();
   max=Math.min(MAX_PLAYERS,Math.max(2,maxPlayers|0));password=pass;me={name:chatText(name)||'Host',outfit};
   const code=roomCode();
   return new Promise((resolve,reject)=>{
    peer=new Peer(PREFIX+code);
    peer.on('open',()=>{role=ROLE.HOST;slot=0;room=code;setLook(0,me);lastTick=now();timer=setTimer(tick,TICK_MS);emit('session',ROLE.HOST,0);resolve(code);});
    peer.on('connection',accept);
    peer.on('call',hostCall);
    peer.on('error',error=>{if(role===ROLE.OFF){stop();reject(error);}else if(error?.type!=='peer-unavailable')failure(error);});
   });
  },
  // Joins a room. The `save` event follows when the host world arrives.
  join({code,name,password:pass='',outfit}){
   stop();
   if(!validCode(code)){emit('status',STATUS.notFound);return;}
   me={name:chatText(name)||'Player',outfit};
   joinStarted=now();
   peer=new Peer();
   peer.on('error',failure);
   peer.on('call',guestCall);
   peer.on('open',()=>{
    role=ROLE.GUEST;room=code;lastTick=now();timer=setTimer(tick,TICK_MS);
    const reliable=peer.connect(PREFIX+code,{label:'reliable',reliable:true,serialization:'binary'});
    host={reliable,pose:null,welcomed:false,ready:false,seen:now(),ping:null,chunks:null,received:0,voices:new Map()};
    reliable.on('open',()=>reliable.send({t:'hello',name:me.name,outfit:me.outfit,build:buildId,exportHash,password:pass}));
    reliable.on('data',guestMessage);
    reliable.on('close',()=>{if(role===ROLE.GUEST)stop(STATUS.hostLeft);});
   });
  },
  // The guest runs the host world. Silence counts as leaving again after this.
  ready(){
   if(role!==ROLE.GUEST||!host?.welcomed||host.ready)return;
   host.ready=true;host.seen=now();host.reliable.send({t:'ready'});
  },
  // The local microphone stream, or null. It keeps across sessions.
  voice(stream){
   if(stream===mic)return;mic=stream;
   if(role===ROLE.HOST){for(const g of guests.values()){if(!g.welcomed)continue;unrelay(g,0);if(mic)relay(g,0,mic);}}
   else if(role===ROLE.GUEST)callHost();
  },
  leave(){
   if(role===ROLE.GUEST&&host?.welcomed)host.reliable.send({t:'leave'});
   stop();
  },
  // The outbound simulation packets of one frame, each with a u16 length. Bit `RELIABLE` of the length picks `reliable`.
  send(outbox){
   if(role===ROLE.OFF||!outbox.length)return;
   // The split copies, because the next frame reuses the outbox buffer.
   const [fast,sure]=splitOutbox(outbox);
   if(fast.length)datagrams(fast,part=>{
    if(role===ROLE.HOST){for(const g of guests.values())if(g.welcomed&&g.pose?.open)g.pose.send(part);}
    else if(host?.pose?.open)host.pose.send(part);
   });
   if(!sure.length)return;
   const message={t:'sim',data:sure};
   if(role===ROLE.HOST){for(const g of guests.values())if(g.welcomed)g.reliable.send(message);}
   else if(host?.welcomed)host.reliable.send(message);
  },
  look(outfit,name=me.name){
   me={name:chatText(name)||me.name,outfit};
   if(role===ROLE.OFF)return;
   setLook(slot,me);
   const message={t:'look',slot,name:me.name,outfit};
   if(role===ROLE.HOST)broadcast(message);else if(host?.welcomed)host.reliable.send(message);
  },
  chat(value){
   const text=chatText(value);if(!text||role===ROLE.OFF)return;
   if(role===ROLE.HOST)broadcast({t:'chat',slot:0,text});else if(host?.welcomed)host.reliable.send({t:'chat',text});
   emit('chat',{name:me.name,text});
  },
 };
}
