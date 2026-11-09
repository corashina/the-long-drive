// Multiplayer screen, chat and name labels. It connects `net.js` with the simulation, the save session and the menu.
import {ROLE,createNet} from './net.js';
import {MODES,createVoiceOutput,talkState} from './voice.js';
import {MOUSE_BUTTONS} from './bindings.js';
import {playerRows} from './room-card.js';
// The source keeps the voice settings with the multiplayer settings (`snl.SaveSettings`), not with `S`.
export const NAME_KEY='thelongdrive.player-name';
// The URL parameter of an invite link. The boot choice then opens the join form with this room code.
export const ROOM_PARAM='room';
const VOICE_KEY='thelongdrive.voice-chat',CHAT_SHOW_MS=10000;

// `reloadJoin(room)` starts the page again to join `room`. `join(room)` joins without a reload, before a world loads. `loading`: the loading card, which shows the join. `audio`: the AudioController.
// `voiceKeys()`: the codes of the `voicechat` action. `media`: `navigator.mediaDevices`.
export function createMultiplayer({document,target,Peer,buildId,exportHash,simulation,saveSession,menu,outfit,storage,reloadJoin,loading,audio,voiceKeys=()=>[],media}){
 const $=selector=>document.querySelector(selector);
 const name=$('#net-name'),hostPassword=$('#net-host-password'),joinPassword=$('#net-join-password'),hostHint=$('#net-host-hint'),session=$('#net-session'),count=$('#net-count'),copyButton=$('#net-code-copy'),linkButton=$('#net-link-copy'),max=$('#net-max'),room=$('#net-room'),code=$('#net-code'),status=$('#net-status');
 const hostButton=$('#net-host-start'),joinButton=$('#net-join-start'),leaveButton=$('#net-leave'),players=$('#net-players');
 const log=$('#net-chat-log'),line=$('#net-chat-line'),labels=$('#net-labels'),labelNodes=new Map();
 const voiceOn=$('#net-voice'),voiceMode=$('#net-voice-mode'),micIcon=$('#net-mic');
 // The settle function of the pending `join(room)`, or null.
 let looksJson=null,names=new Map(),joining=null;
 try{name.value=storage()?.getItem(NAME_KEY)??'';}catch{}
 const storeName=()=>{try{storage()?.setItem(NAME_KEY,name.value);}catch{}};
 // Voice chat starts off, so the game asks for the microphone only after the player turns it on. Source `ResetSettings` has it on.
 let voice={on:false,mode:MODES[0]};
 try{const v=JSON.parse(storage()?.getItem(VOICE_KEY)??'null');if(typeof v?.on==='boolean'&&MODES.includes(v.mode))voice=v;}catch{}
 voiceOn.checked=voice.on;voiceMode.value=voice.mode;
 const output=createVoiceOutput({audio});
 let mic=null,micPending=false,talking=false,held=new Set(),pressed=false;
 name.addEventListener('change',()=>{storeName();net.look(outfit.outfit,name.value);});
 function paint(){
  const off=net.role===ROLE.OFF;
  hostButton.disabled=!off||!menu.model.started||menu.model.pending;
  hostHint.hidden=menu.model.started||!off;
  session.hidden=!net.code;leaveButton.hidden=!net.code;if(code.textContent!==net.code){code.textContent=net.code;copyButton.textContent='Copy code';linkButton.textContent='Copy link';}
  joinButton.disabled=!off||menu.model.pending;
  for(const node of [name,hostPassword,max,room,joinPassword])node.disabled=!off;
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
  packets:batch=>simulation.netPackets(batch),
  voice:(slot,stream)=>{if(voice.on||!stream)output.stream(slot,stream);},
  leave:slot=>{simulation.netLeave(slot);labelNodes.get(slot)?.remove();labelNodes.delete(slot);},
  looks:looks=>{
   names=new Map([...looks].map(([s,l])=>[s,l.name]));
   looksJson=JSON.stringify([...looks].filter(([s,l])=>s!==net.slot&&l.outfit).map(([s,l])=>[s,l.outfit]));
   loading.players([...looks].sort(([a],[b])=>a-b).map(([s,l])=>({name:l.name,host:s===0,you:s===net.slot})));
  },
  download:(chunks,total)=>{loading.step('connect',1);loading.step('host',chunks,total,Math.floor(100*chunks/total)+'%');},
  chat,
  // A failed `join(room)` goes back to the boot choice, which shows why.
  status:text=>{if(joining&&net.role===ROLE.OFF){const settle=joining;joining=null;settle({error:text});}else say(text);},
  peers:list=>{
   count.textContent=`${list.length} / ${net.max}`;
   players.replaceChildren(...playerRows(document,list.sort((a,b)=>a.slot-b.slot).map(p=>({name:p.name,host:p.slot===0,you:p.slot===net.slot,ping:p.ping}))));
  },
  session:(role,slot)=>{
   simulation.netSession(role,slot,packets=>net.send(packets));
   if(role===ROLE.OFF){for(const node of labelNodes.values())node.remove();labelNodes.clear();looksJson='[]';output.clear();}
   paint();
  },
  save:bytes=>{
   if(!joining)return;
   const settle=joining;joining=null;settle({save:bytes});paint();
  },
 }});
 // Rust reads the peer outfits after each change, like `gameOutfit`.
 target.gameRemoteLooks=()=>{const json=looksJson;looksJson=null;return json??undefined;};
 target.gameRemoteLabel=label;
 target.gameRemoteVoice=(slot,x,y,z,alive)=>output.place(slot,x,y,z,alive);
 hostButton.addEventListener('click',async()=>{
  storeName();say('Opening a room…');
  try{await net.host({maxPlayers:Number(max.value),password:hostPassword.value,name:name.value,outfit:outfit.outfit});say('');}
  catch{say('Cannot reach the connection server.');}
 });
 joinButton.addEventListener('click',()=>{storeName();say('Connecting…');reloadJoin({code:room.value.trim().toUpperCase(),name:name.value,password:joinPassword.value});});
 for(const node of [room,joinPassword])node.addEventListener('keydown',event=>{if(event.code==='Enter'&&!joinButton.disabled)joinButton.click();});
 // The button shows "Copied" for a moment, then its label again.
 const copy=(button,text,label)=>{document.defaultView?.navigator?.clipboard?.writeText(text).then(()=>{button.textContent='Copied';setTimeout(()=>{button.textContent=label;},1500);},()=>{});};
 copyButton.addEventListener('click',()=>copy(copyButton,net.code,'Copy code'));
 // The invite link opens the boot choice with the room code filled in.
 linkButton.addEventListener('click',()=>{const url=new URL(document.defaultView.location.href);url.search='';url.hash='';url.searchParams.set(ROOM_PARAM,net.code);copy(linkButton,url.href,'Copy link');});
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
  // Joins `{code,name,password}` before a world loads. Resolves with `{save}` (the host world JSON) or `{error}`.
  join(value){
   name.value=value.name;room.value=value.code;joinPassword.value=value.password;
   loading.join(value.code);loading.step('connect',0);
   const done=new Promise(resolve=>{joining=resolve;});
   net.join({...value,outfit:outfit.outfit});
   return done;
  },
  leave:()=>net.leave(),
  ready:()=>net.ready(),
  paint,
 };
}
