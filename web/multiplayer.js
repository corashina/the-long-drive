// Multiplayer screen, chat and name labels. It connects `net.js` with the simulation, the save session and the menu.
import {ROLE,createNet} from './net.js';
import {MODES,createVoiceOutput,talkState} from './voice.js';
import {MOUSE_BUTTONS} from './bindings.js';
// The source keeps the voice settings with the multiplayer settings (`snl.SaveSettings`), not with `S`.
const NAME_KEY='the-long-slop.player-name',VOICE_KEY='the-long-slop.voice-chat',CHAT_SHOW_MS=10000;

// `room`: the room of a Join reload. `joined(save,error)` takes its host world JSON, or null and why.
// `reloadJoin(room)` starts the page again to join `room`. `audio`: the AudioController.
// `voiceKeys()`: the codes of the `voicechat` action. `media`: `navigator.mediaDevices`.
export function createMultiplayer({document,target,Peer,buildId,exportHash,simulation,saveSession,menu,outfit,storage,room:joinRoom=null,joined=()=>{},reloadJoin,audio,voiceKeys=()=>[],media}){
 const $=selector=>document.querySelector(selector);
 const name=$('#net-name'),password=$('#net-password'),max=$('#net-max'),room=$('#net-room'),code=$('#net-code'),status=$('#net-status');
 const hostButton=$('#net-host-start'),joinButton=$('#net-join-start'),leaveButton=$('#net-leave'),players=$('#net-players');
 const log=$('#net-chat-log'),line=$('#net-chat-line'),labels=$('#net-labels'),labelNodes=new Map();
 const voiceOn=$('#net-voice'),voiceMode=$('#net-voice-mode'),micIcon=$('#net-mic');
 let looksJson=null,names=new Map();
 try{name.value=storage()?.getItem(NAME_KEY)??'';}catch{}
 const storeName=()=>{try{storage()?.setItem(NAME_KEY,name.value);}catch{}};
 // Source `ResetSettings`: voice chat on, push to talk.
 let voice={on:true,mode:MODES[0]};
 try{const v=JSON.parse(storage()?.getItem(VOICE_KEY)??'null');if(typeof v?.on==='boolean'&&MODES.includes(v.mode))voice=v;}catch{}
 voiceOn.checked=voice.on;voiceMode.value=voice.mode;
 const output=createVoiceOutput({audio});
 let mic=null,micPending=false,talking=false,held=new Set(),pressed=false;
 name.addEventListener('change',()=>{storeName();net.look(outfit.outfit,name.value);});
 function paint(){
  const off=net.role===ROLE.OFF;
  hostButton.disabled=!off||!menu.model.started||menu.model.pending;
  joinButton.disabled=!off||menu.model.pending;
  leaveButton.hidden=off;
  for(const node of [name,password,max,room])node.disabled=!off;
  if(off)code.textContent='-';
  syncMic();
 }
 // Source `VoiceChatUpdate`: the microphone records while voice chat is on in a session.
 function syncMic(){
  const want=voice.on&&net.role!==ROLE.OFF;
  if(want&&!mic&&!micPending&&media?.getUserMedia){
   micPending=true;
   media.getUserMedia({audio:{echoCancellation:true,noiseSuppression:true}}).then(stream=>{
    micPending=false;
    if(!voice.on||net.role===ROLE.OFF){stream.getTracks().forEach(t=>t.stop());return;}
    mic=stream;setTalking(talking);net.voice(mic);
   },()=>{micPending=false;voice.on=false;voiceOn.checked=false;chat({name:'',text:'Microphone not available.'});});
  }else if(!want&&mic){net.voice(null);mic.getTracks().forEach(t=>t.stop());mic=null;setTalking(false);}
  if(!voice.on)output.clear();
 }
 function setTalking(value){talking=value;mic?.getAudioTracks().forEach(t=>{t.enabled=value;});micIcon.hidden=!(value&&mic);}
 function storeVoice(){try{storage()?.setItem(VOICE_KEY,JSON.stringify(voice));}catch{}}
 voiceOn.addEventListener('change',()=>{voice={...voice,on:voiceOn.checked};storeVoice();syncMic();});
 voiceMode.addEventListener('change',()=>{if(!MODES.includes(voiceMode.value))return;voice={...voice,mode:voiceMode.value};storeVoice();});
 const voiceKey=code=>voiceKeys().includes(code);
 const keyDown=code=>{if(!voiceKey(code)||held.has(code))return;held.add(code);pressed=true;};
 const keyUp=code=>{held.delete(code);};
 target.addEventListener('keydown',event=>{if(!event.repeat)keyDown(event.code);});
 target.addEventListener('keyup',event=>keyUp(event.code));
 target.addEventListener('mousedown',event=>keyDown(MOUSE_BUTTONS[event.button]));
 target.addEventListener('mouseup',event=>keyUp(MOUSE_BUTTONS[event.button]));
 target.addEventListener('blur',()=>held.clear());
 function say(text){status.textContent=text;menu.model.status=text;menu.paint();paint();}
 function chat({name:from,text}){
  const item=document.createElement('li');item.textContent=from?`${from}: ${text}`:text;log.append(item);
  setTimeout(()=>item.remove(),CHAT_SHOW_MS);
  while(log.children.length>8)log.firstElementChild.remove();
 }
 function label(slot,x,y){
  let node=labelNodes.get(slot);
  if(x<0){if(node)node.hidden=true;return;}
  if(!node){node=document.createElement('span');labelNodes.set(slot,node);labels.append(node);}
  node.textContent=names.get(slot)??'';node.hidden=false;node.classList.toggle('speaking',output.speaking(slot));node.style.transform=`translate(${x}px,${y}px) translate(-50%,-100%)`;
 }
 const net=createNet({Peer,buildId,exportHash,capture:()=>saveSession.share(),on:{
  packet:packet=>simulation.netPacket(packet),
  voice:(slot,stream)=>{if(voice.on||!stream)output.stream(slot,stream);},
  leave:slot=>{simulation.netLeave(slot);labelNodes.get(slot)?.remove();labelNodes.delete(slot);},
  looks:looks=>{
   names=new Map([...looks].map(([s,l])=>[s,l.name]));
   looksJson=JSON.stringify([...looks].filter(([s,l])=>s!==net.slot&&l.outfit).map(([s,l])=>[s,l.outfit]));
  },
  chat,
  status:text=>{say(text);if(joinRoom&&net.role===ROLE.OFF){joinRoom=null;joined(null,text);}},
  peers:list=>{players.replaceChildren(...list.map(p=>{const item=document.createElement('li');item.textContent=p.ping===null?p.name:`${p.name} ${p.ping} ms`;return item;}));},
  session:(role,slot)=>{
   simulation.netSession(role,slot,packets=>net.send(packets));
   if(role===ROLE.OFF){for(const node of labelNodes.values())node.remove();labelNodes.clear();looksJson='[]';output.clear();}
   paint();
  },
  save:bytes=>{
   if(!joinRoom)return;
   joinRoom=null;joined(bytes,'');paint();
  },
 }});
 // Rust reads the peer outfits after each change, like `gameOutfit`.
 target.gameRemoteLooks=()=>{const json=looksJson;looksJson=null;return json??undefined;};
 target.gameRemoteLabel=label;
 target.gameRemoteVoice=(slot,x,y,z,alive)=>output.place(slot,x,y,z,alive);
 hostButton.addEventListener('click',async()=>{
  storeName();say('Opening a room...');
  try{const value=await net.host({maxPlayers:Number(max.value),password:password.value,name:name.value,outfit:outfit.outfit});code.textContent=value;say(`Room code: ${value}`);}
  catch{say('Cannot reach the connection server.');}
 });
 joinButton.addEventListener('click',()=>{storeName();say('Connecting...');reloadJoin({code:room.value.trim().toUpperCase(),name:name.value,password:password.value});});
 leaveButton.addEventListener('click',()=>{net.leave();say('Left the game.');});
 // Enter opens the chat line while playing. Enter sends it, Escape closes it.
 target.addEventListener('keydown',event=>{
  if(net.role===ROLE.OFF||event.code!=='Enter'||event.repeat)return;
  if(line.hidden){if(menu.model.state!=='playing')return;event.preventDefault();line.hidden=false;line.focus();return;}
 });
 line.addEventListener('keydown',event=>{
  event.stopPropagation();
  if(event.code==='Enter'){event.preventDefault();net.chat(line.value);line.value='';line.hidden=true;}
  else if(event.code==='Escape'){event.preventDefault();line.value='';line.hidden=true;}
 });
 target.addEventListener('pagehide',()=>net.leave());
 for(const id of ['#multiplayer','#start-multiplayer'])$(id).addEventListener('click',paint);
 paint();
 if(joinRoom){room.value=joinRoom.code;password.value=joinRoom.password;say('Connecting...');net.join({...joinRoom,outfit:outfit.outfit});}
 return {
  get role(){return net.role;},
  // The game input pauses while the chat line is open.
  typing:()=>!line.hidden,
  // Runs once per frame. `playing`: the game has the input (source `noInput()` is false).
  frame(playing){
   if(net.role===ROLE.OFF)return;
   const blocked=!playing||!line.hidden;
   const next=talkState(voice.mode,talking,{held:held.size>0,pressed,blocked});pressed=false;
   if(next!==talking)setTalking(next);
   output.frame();
  },
  look:value=>net.look(value,name.value),
  leave:()=>net.leave(),
  ready:()=>net.ready(),
  paint,
 };
}
