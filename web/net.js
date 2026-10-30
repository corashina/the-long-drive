// Multiplayer peers of source `sns` and `snl`. PeerJS runs on the page thread: Chrome has no WebRTC in workers.
// Star like the source: guests talk only to the host. The host forwards guest messages to the other guests.
// Each guest has a `reliable` connection for the session messages and the save, and a `pose` connection for
// the simulation packets. A packet is the type byte, the sender slot byte, then a body that only Rust reads.
// A guest loads the host world for many seconds with a busy page thread, so silence until `ready` uses `LOAD_TIMEOUT_MS`.
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
// Cuts a chat line to `CHAT_BYTES` UTF-8 bytes, like the source chat field limit.
export function chatText(text){
 const encoded=new TextEncoder().encode(String(text).trim());
 return new TextDecoder().decode(encoded.subarray(0,CHAT_BYTES)).replace(/�$/,'');
}

// `Peer`: the PeerJS class. `capture()`: the host save record as a string.
// Events: `packet(bytes)`, `leave(slot)`, `looks(Map slot→{name,outfit})`, `chat({name,text})`, `status(text)`,
// `save(bytes)`, `session(role,slot)`, `peers([{slot,name,ping}])`.
export function createNet({Peer,buildId,exportHash,capture,on,now=()=>performance.now(),setTimer=setInterval,clearTimer=clearInterval}){
 let role=ROLE.OFF,slot=0,peer=null,timer=null,joinStarted=0,lastTick=0,me={name:'',outfit:null};
 let max=MAX_PLAYERS,password='',host=null;
 const guests=new Map(),looks=new Map();
 const emit=(name,...args)=>on[name]?.(...args);
 const peers=()=>emit('peers',[...looks].map(([s,l])=>({slot:s,name:l.name,ping:s===slot?null:(role===ROLE.HOST?guestBySlot(s)?.ping:host?.ping)??null})));
 const guestBySlot=s=>[...guests.values()].find(g=>g.slot===s);
 const setLook=(s,look)=>{if(look)looks.set(s,look);else looks.delete(s);emit('looks',new Map(looks));peers();};
 function stop(status){
  // Role first: the closes below fire close events that must not stop again.
  const was=role;role=ROLE.OFF;slot=0;looks.clear();
  if(timer!==null)clearTimer(timer);timer=null;
  const closing=[...guests.values()];guests.clear();
  for(const g of closing){g.reliable?.close();g.pose?.close();}
  const old=host;host=null;old?.reliable?.close();old?.pose?.close();
  peer?.destroy();peer=null;
  if(was!==ROLE.OFF)emit('session',ROLE.OFF,0);
  if(status)emit('status',status);
 }
 function failure(error){
  const type=error?.type;
  if(type==='peer-unavailable')stop(STATUS.notFound);
  else if(type==='unavailable-id')stop(STATUS.server);
  else if(['network','server-error','socket-error','browser-incompatible'].includes(type))stop(STATUS.server);
  else stop(error?.message??String(error));
 }
 // Host: send to every guest except `except`.
 function broadcast(message,except=-1){for(const g of guests.values())if(g.slot!==except&&g.welcomed)g.reliable.send(message);}
 function removeGuest(id){
  const g=guests.get(id);if(!g)return;
  guests.delete(id);g.reliable?.close();g.pose?.close();
  if(g.welcomed){broadcast({t:'leave',slot:g.slot});emit('leave',g.slot);setLook(g.slot,null);}
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
  try{save=new TextEncoder().encode(await capture());}catch(error){return refuse('save');}
  if(!guests.has(g.id))return;
  const chunks=Math.ceil(save.length/CHUNK);
  g.reliable.send({t:'welcome',slot:free,chunks,looks:[...looks]});
  for(let i=0;i<chunks;i++)g.reliable.send({t:'save',i,data:save.subarray(i*CHUNK,(i+1)*CHUNK)});
  g.welcomed=true;
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
 }
 function hostPose(g,data){
  g.seen=now();
  if(!g.welcomed)return;
  const packet=bytes(data).slice();
  if(packet.length<2)return;
  // The host stamps the sender slot, so a guest cannot speak for another.
  packet[1]=g.slot;
  for(const other of guests.values())if(other!==g&&other.welcomed&&other.pose?.open)other.pose.send(packet);
  emit('packet',packet);
 }
 function accept(conn){
  const id=conn.peer;
  if(conn.label==='pose'){
   const g=guests.get(id);if(!g){conn.close();return;}
   g.pose=conn;conn.on('data',data=>hostPose(g,data));return;
  }
  if(guests.has(id)){conn.close();return;}
  const g={id,reliable:conn,pose:null,slot:undefined,welcomed:false,ready:false,seen:now(),ping:null};
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
   host.pose.on('data',data=>{host.seen=now();emit('packet',bytes(data));});
   if(m.chunks===0)emit('status',STATUS.save);
   return;
  }
  if(m.t==='save'&&host.chunks&&host.chunks[m.i]===undefined){
   host.chunks[m.i]=bytes(m.data);host.received++;
   emit('status',`Loading the host world... ${Math.round(100*host.received/host.chunks.length)}%`);
   if(host.received===host.chunks.length){
    const save=new Uint8Array(host.chunks.reduce((n,c)=>n+c.length,0));let at=0;
    for(const c of host.chunks){save.set(c,at);at+=c.length;}
    host.chunks=null;emit('session',ROLE.GUEST,slot);emit('save',save);peers();
   }
   return;
  }
  if(m.t==='look')setLook(m.slot,{name:m.name,outfit:m.outfit});
  else if(m.t==='chat')emit('chat',{name:looks.get(m.slot)?.name??'',text:chatText(m.text)});
  else if(m.t==='leave'){emit('leave',m.slot);setLook(m.slot,null);}
  else if(m.t==='ping')host.reliable.send({t:'pong',at:m.at});
  else if(m.t==='pong'){host.ping=Math.round(now()-m.at);peers();}
 }
 return {
  get role(){return role;},
  get slot(){return slot;},
  // Opens a room. Resolves with the room code.
  host({maxPlayers=MAX_PLAYERS,password:pass='',name,outfit}){
   stop();
   max=Math.min(MAX_PLAYERS,Math.max(2,maxPlayers|0));password=pass;me={name:chatText(name)||'Host',outfit};
   const code=roomCode();
   return new Promise((resolve,reject)=>{
    peer=new Peer(PREFIX+code);
    peer.on('open',()=>{role=ROLE.HOST;slot=0;setLook(0,me);lastTick=now();timer=setTimer(tick,TICK_MS);emit('session',ROLE.HOST,0);resolve(code);});
    peer.on('connection',accept);
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
   peer.on('open',()=>{
    role=ROLE.GUEST;lastTick=now();timer=setTimer(tick,TICK_MS);
    const reliable=peer.connect(PREFIX+code,{label:'reliable',reliable:true,serialization:'binary'});
    host={reliable,pose:null,welcomed:false,ready:false,seen:now(),ping:null,chunks:null,received:0};
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
  leave(){
   if(role===ROLE.GUEST&&host?.welcomed)host.reliable.send({t:'leave'});
   stop();
  },
  // Outbound simulation packets of one frame, each with a u16 length.
  send(packets){
   if(role===ROLE.OFF||!packets.length)return;
   for(let at=0;at+2<=packets.length;){
    const n=packets[at]|packets[at+1]<<8,packet=packets.subarray(at+2,at+2+n);at+=2+n;
    if(role===ROLE.HOST){for(const g of guests.values())if(g.welcomed&&g.pose?.open)g.pose.send(packet);}
    else if(host?.pose?.open)host.pose.send(packet);
   }
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
