import {ClipCache} from './audio/clips.js';
import {VoiceBank,PORTABLE_RADIO_EMITTER,PORTABLE_RADIO_SLOTS} from './audio/voices.js';
export const sourceGainDb=value=>20*Math.log(Math.max(.001,Math.min(1,value)));
export class AudioController {
 constructor(raw){this.raw=raw;this.soundGain=1;this.session=1;this.sequence=0;this.context=null;this.bank=null;this.cache=null;this.mixers=new Map();this.disabled=false;this.paused=true;this.started=false;this.disposed=false;this.epoch=0;this.diagnostics=new Set();this.unlocking=false;this.awaitingPlaying=false;this.accepted=true;try{if(raw.schema_version!==1||!raw.clips.length||!raw.voices.length||!raw.bindings.length||!raw.mixers.length||!raw.shared.settings)throw Error('Invalid audio data');this.bindings=new Map(raw.bindings.map(b=>[b.id,b]));this.radios=[];for(let i=0;i<raw.portable_radios.length*PORTABLE_RADIO_SLOTS;i++){const base=PORTABLE_RADIO_EMITTER+i*4;this.radios.push({emitter:PORTABLE_RADIO_EMITTER+i,voices:[base+1,base+2,base+3],current:[null,null],next:[null,null],seen:0});}this.radioDemand=new Array(this.radios.length*4).fill(null);}catch(error){this.fail(error);}}
 fail(error){if(this.disposed)return;this.disabled=true;this.paused=true;try{this.bank?.pause();this.context?.suspend().catch(()=>{});}catch{}const message=error?.message??String(error);if(!this.diagnostics.has(message)){this.diagnostics.add(message);console.warn('Audio disabled: '+message);}}
 clipFailure(id,error){if(this.disposed)return;const message='Audio clip '+id+' disabled: '+(error?.message??String(error));if(!this.diagnostics.has(message)){this.diagnostics.add(message);console.warn(message);}}
 sourceFailure(error,s){if(this.disposed)return;if(this.context?.state==='closed'){this.fail(error);return;}this.bank.disable(s);const kind=s.event?'binding':'voice',id=s.event?s.binding:s.record.id,message='Audio '+kind+' '+id+' disabled: '+(error?.message??String(error));if(!this.diagnostics.has(message)){this.diagnostics.add(message);console.warn(message);}}
 initialize(){this.context=new AudioContext();const ctx=this.context;this.master=ctx.createGain();this.master.connect(ctx.destination);this.ear=ctx.createGain();this.ear.connect(this.master);this.ring={gain:ctx.createGain(),pan:ctx.createStereoPanner(),source:null,loading:false,token:0};this.ring.gain.connect(this.ring.pan);this.ring.pan.connect(this.master);for(const m of this.raw.mixers){const gain=ctx.createGain();gain.gain.value=m.mute?0:10**(m.gain_db/20);this.mixers.set(m.id,gain);}for(const m of this.raw.mixers){let node=this.mixers.get(m.id);if(!m.bypass_effects)for(const effect of m.effects){if(effect.type!==10||effect.bypass)continue;const eq=ctx.createBiquadFilter();eq.type='peaking';eq.frequency.value=effect.parameters[0];eq.Q.value=1/effect.parameters[1];eq.gain.value=40*Math.log10(effect.parameters[2]);node.connect(eq);node=eq;}node.connect(m.parent?this.mixers.get(m.parent):this.ear);}this.cache=new ClipCache(ctx,this.raw,()=>this.session,(id,error)=>this.clipFailure(id,error));this.bank=new VoiceBank(ctx,this.raw,this.cache,this.mixers,()=>this.session,()=>this.started&&!this.paused&&!this.disabled&&!this.disposed,(error,slot)=>slot?this.sourceFailure(error,slot):this.fail(error));}
 unlock(){if(this.disposed||this.disabled||this.unlocking)return;const epoch=this.epoch;try{if(!this.context)this.initialize();this.unlocking=true;this.awaitingPlaying=true;const pending=this.context.resume();Promise.resolve(pending).then(()=>{if(epoch!==this.epoch||this.disposed)return;this.unlocking=false;this.paused=false;this.bank.resume();}).catch(error=>{if(epoch===this.epoch){this.unlocking=false;this.fail(error);}});}catch(error){this.unlocking=false;this.fail(error);}}
 begin(session,started,paused,gameSeconds){this.accepted=session>=this.session;if(!this.accepted)return;if(session!==this.session)this.restart(session);this.started=started;this.gameSeconds=gameSeconds;if(started&&!paused)this.awaitingPlaying=false;else if(paused&&!this.awaitingPlaying)this.pause();this.bank?.begin();}
 originShift(x,y,z){if(!this.accepted)return;this.bank?.originShift(x,y,z);}
 listener(position,rotation){if(!this.accepted)return;this.bank?.listener(position,rotation);}
 emitter(id,position,rotation,alive){if(!this.accepted)return;this.bank?.emitter(id,position,rotation,alive);const r=this.radios?.find(r=>r.emitter===id);if(!alive&&r&&this.bank){for(const voice of r.voices)this.bank.stop(this.bank.voices.get(voice),false);r.current.fill(null);r.next.fill(null);r.seen=0;}}
 voice(id,playing,gain,pitch,looping,clip){try{if(this.accepted&&!this.disabled)this.bank?.update(id,playing,gain,pitch,looping,clip);}catch(error){this.fail(error);}}
 event(session,sequence,binding,emitter,x,y,z,gain,pitch,timeSeconds){if(session!==this.session||sequence<=this.sequence)return;this.sequence=sequence;if(this.paused||this.disabled||!this.started)return;const record=this.bindings?.get(binding);if(!record)return;try{this.bank?.event(record,emitter,x,y,z,gain,pitch);}catch(error){this.fail(error);}}
 mixer(id,sourceDb){const mixer=this.mixers.get(id);if(mixer)mixer.gain.value=10**(sourceDb/20);}
 route(id,mixer){this.bank?.route(id,mixer);}
 radio(emitter,frame){
  if(!this.accepted)return;
  const r=this.radios.find(r=>r.emitter===emitter);if(!r)return;
  if(!this.bank||this.disabled||this.disposed)return;
  const alive=this.bank.emitters.get(r.emitter)?.alive,timing=this.raw.radio;
  const playing=frame.power&&alive&&this.started&&!this.paused;
  r.seen=this.bank.frame;
  this.bank.update(r.voices[0],playing,frame.staticGain,1,true);
  for(let i=0;i<2;i++){
   const p=i===0?frame.primary:frame.secondary,s=this.bank.voices.get(r.voices[i+1]);
   const clip=playing?p.clip:null;
   if(s.clip!==clip){this.bank.stop(s,false);s.clip=clip;}
   s.seen=this.bank.frame;s.playing=playing&&clip!==null&&!this.bank.failedVoices.has(s.record.id);s.looping=false;s.volume=p.gain;s.pitch=1;
   r.current[i]=s.playing?clip:null;r.next[i]=s.playing?p.nextClip:null;
   if(!s.playing){this.bank.stop(s,false);continue;}
   this.bank.parameters(s);
   const buffer=this.cache.radioBuffers.get(s.clip);if(!buffer)continue;
   const offset=p.offsetSeconds<buffer.duration?p.offsetSeconds:Math.max(0,buffer.duration-timing.late_seek_margin_seconds);
   if(s.source&&Math.abs(s.offset-offset)>timing.time_correction_seconds){this.bank.stop(s,false);s.playing=true;}
   if(!s.source){s.buffer=buffer;s.offset=offset;this.bank.connect(s,this.bank.routes.get(s.record.id)??s.record.mixer);this.bank.start(s);}
  }
 }
 // Radio clips load in demand order: every heard station, then the neighbours, then the next recordings.
 radioCache(){
  const demand=this.radioDemand,count=this.radios.length;
  for(let i=0;i<count;i++){const r=this.radios[i],seen=r.seen===this.bank.frame;demand[i]=seen?r.current[0]:null;demand[count+i]=seen?r.current[1]:null;demand[2*count+i]=seen?r.next[0]:null;demand[3*count+i]=seen?r.next[1]:null;}
  this.cache.radio(demand,this.session);
 }
 finish(){if(!this.accepted)return;try{if(this.bank){this.radioCache();this.bank.finish();}}catch(error){this.fail(error);}}
 pause(){this.awaitingPlaying=false;if(this.paused&&!this.unlocking)return;this.epoch++;this.unlocking=false;this.paused=true;this.bank?.pause();this.stopRing();try{this.context?.suspend().catch(()=>{});}catch(error){this.fail(error);}}
 // Source `MasterVolume`: `FSound` in decibels. The survival fade multiplies it.
 volume(sound){this.soundGain=10**(sourceGainDb(sound)/20);}
 survivalMaster(gain,revision,seconds){if(!this.accepted)return;if(this.master)this.master.gain.value=gain*this.soundGain;if(this.bank){this.bank.clock(seconds);this.bank.motionRevision(revision);}}
 // Mixer PlayerEar over the game mixers. The ear ring loop plays outside it (fpscontroller EarRingSource).
 playerEar(db,clip,ring,pan){if(!this.accepted||!this.ear)return;this.ear.gain.value=10**(db/20);const r=this.ring;r.gain.gain.value=ring;r.pan.pan.value=pan;if(ring<=0||this.paused||this.disabled||!this.started||this.cache.failed.has(clip)){this.stopRing();return;}if(r.source||r.loading)return;r.loading=true;const token=++r.token,session=this.session;this.cache.effect(clip,session).then(buffer=>{if(token!==r.token)return;r.loading=false;if(!buffer||session!==this.session)return;const source=this.context.createBufferSource();source.buffer=buffer;source.loop=true;source.connect(r.gain);source.start();r.source=source;}).catch(error=>{if(token===r.token)r.loading=false;this.clipFailure(clip,error);});}
 stopRing(){const r=this.ring;if(!r)return;r.token++;r.loading=false;if(r.source){try{r.source.stop();}catch{}r.source.disconnect();r.source=null;}}
 survivalEmitter(kind,x,y,z){this.bank?.survivalEmitter(kind,x,y,z);}
 survival(kind,emitter,action,clip,gain,pitch,x,y,z){try{if(this.accepted&&!this.disabled&&!this.paused)this.bank?.survival(kind,emitter,action,clip,gain,pitch,x,y,z);}catch(error){this.fail(error);}}
 restart(session){this.epoch++;this.unlocking=false;this.session=session;this.sequence=0;this.awaitingPlaying=false;this.disabled=false;this.paused=true;this.diagnostics.clear();this.cache?.reset();this.bank?.reset();this.stopRing();try{this.context?.suspend().catch(()=>{});}catch(error){this.fail(error);}if(!this.bindings)this.disabled=true;}
 dispose(){this.disposed=true;this.epoch++;this.stopRing();this.cache?.dispose();this.bank?.dispose();try{this.context?.close().catch(()=>{});}catch{}this.mixers.clear();}
}
export function createAudio(raw){return new AudioController(raw);}
// Opcodes of `gameAudioFrame`. Keep in step with src/app/audio.rs. Arguments follow each opcode.
export const OP={BEGIN:0,SURVIVAL_MASTER:1,PLAYER_EAR:2,ORIGIN_SHIFT:3,LISTENER:4,SURVIVAL_EMITTER:5,SURVIVAL:6,EMITTER:7,VOICE:8,EVENT:9,MIXER:10,ROUTE:11,RADIO:12,END:13};
// One Float64Array per frame. It is a view of WASM memory, so read it now and keep no reference.
export function installAudioBridge(target,audio){
 const position=new Float32Array(3),rotation=new Float32Array(4),frame={power:false,am:false,frequency:0,volume:0,staticGain:0,primary:{clip:null,offsetSeconds:0,gain:0,nextClip:null},secondary:{clip:null,offsetSeconds:0,gain:0,nextClip:null}};
 const pose=(b,i)=>{position[0]=b[i];position[1]=b[i+1];position[2]=b[i+2];rotation[0]=b[i+3];rotation[1]=b[i+4];rotation[2]=b[i+5];rotation[3]=b[i+6];};
 const playback=(p,b,i)=>{p.clip=b[i]||null;p.offsetSeconds=b[i+1];p.gain=b[i+2];p.nextClip=b[i+3]||null;};
 target.gameAudioError=message=>audio.fail(Error(message));
 target.gameAudioFrame=b=>{
  let i=0;const n=b.length;
  while(i<n){
   switch(b[i++]){
    case OP.BEGIN:audio.begin(b[i],b[i+1]!==0,b[i+2]!==0,b[i+3]);i+=4;break;
    case OP.SURVIVAL_MASTER:audio.survivalMaster(b[i],b[i+1],b[i+2]);i+=3;break;
    case OP.PLAYER_EAR:audio.playerEar(b[i],b[i+1],b[i+2],b[i+3]);i+=4;break;
    case OP.ORIGIN_SHIFT:audio.originShift(b[i],b[i+1],b[i+2]);i+=3;break;
    case OP.LISTENER:pose(b,i);audio.listener(position,rotation);i+=7;break;
    case OP.SURVIVAL_EMITTER:audio.survivalEmitter(b[i],b[i+1],b[i+2],b[i+3]);i+=4;break;
    case OP.SURVIVAL:audio.survival(b[i],b[i+1],b[i+2],b[i+3],b[i+4],b[i+5],b[i+6],b[i+7],b[i+8]);i+=9;break;
    case OP.EMITTER:pose(b,i+1);audio.emitter(b[i],position,rotation,b[i+8]!==0);i+=9;break;
    case OP.VOICE:audio.voice(b[i],b[i+1]!==0,b[i+2],b[i+3],b[i+4]!==0,b[i+5]);i+=6;break;
    case OP.EVENT:audio.event(b[i],b[i+1],b[i+2],b[i+3],b[i+4],b[i+5],b[i+6],b[i+7],b[i+8],b[i+9]);i+=10;break;
    case OP.MIXER:audio.mixer(b[i],b[i+1]);i+=2;break;
    case OP.ROUTE:audio.route(b[i],b[i+1]);i+=2;break;
    case OP.RADIO:frame.power=b[i+1]!==0;frame.am=b[i+2]!==0;frame.frequency=b[i+3];frame.volume=b[i+4];frame.staticGain=b[i+5];playback(frame.primary,b,i+6);playback(frame.secondary,b,i+10);audio.radio(b[i],frame);i+=14;break;
    case OP.END:audio.finish();break;
    default:audio.fail(Error('Audio frame opcode '+b[i-1]+' unknown'));return;
   }
  }
 };
}
