// Voice chat of source `snl.VoiceChatUpdate` and `snTempPlayerSync.Voice`. `net.js` carries the streams.
// Each remote voice plays in 3D at its figure head (source `mpplayerscript.SVoice`).
// Modes of source `voiceChatSetting`. Push to talk is the source default.
export const MODES=['pushToTalk','toggle','continous'];
// Source `DoHide`. The remote voice icon shows for 1 s after the voice.
const SPEAKING_MS=1000;
// RMS level above which a remote stream counts as voice.
const SPEAKING_LEVEL=0.01;

// Source `VoiceChatUpdate`: the next talk state. `held`/`pressed`: the `voicechat` key. `blocked`: source `noInput()`.
export function talkState(mode,talking,{held,pressed,blocked}){
 if(mode==='continous')return true;
 if(mode==='pushToTalk')return held&&!blocked;
 return pressed&&!blocked?!talking:talking;
}

// `audio`: the AudioController (its context, master gain and listener). `now()`: milliseconds.
export function createVoiceOutput({audio,now=()=>performance.now()}){
 const voices=new Map(),relative=new Float32Array(3);
 let samples=null;
 function node(slot){
  const v=voices.get(slot);
  if(!v||v.panner||!audio.context||!audio.master)return v;
  const ctx=audio.context;
  v.source=ctx.createMediaStreamSource(v.stream);
  v.panner=ctx.createPanner();v.panner.panningModel='HRTF';v.panner.distanceModel='inverse';
  v.analyser=ctx.createAnalyser();v.analyser.fftSize=256;
  v.gain=ctx.createGain();
  v.source.connect(v.analyser);v.source.connect(v.panner);v.panner.connect(v.gain);v.gain.connect(audio.master);
  // Chrome plays a WebRTC stream in Web Audio only while a media element also holds it.
  v.element=new Audio();v.element.muted=true;v.element.srcObject=v.stream;v.element.play?.().catch(()=>{});
  return v;
 }
 function drop(v){v.source?.disconnect();v.panner?.disconnect();v.gain?.disconnect();if(v.element)v.element.srcObject=null;}
 return {
  // A remote stream, or null when it ends.
  stream(slot,stream){
   const old=voices.get(slot);if(old)drop(old);voices.delete(slot);
   if(stream)voices.set(slot,{stream,source:null,panner:null,gain:null,analyser:null,element:null,position:new Float32Array(3),placed:false,heard:-Infinity});
  },
  // The figure head of `slot` in listener space (`gameAudioListener`). `alive` false: the figure is gone.
  place(slot,x,y,z,alive){const v=voices.get(slot);if(!v)return;v.placed=alive;v.position[0]=x;v.position[1]=y;v.position[2]=z;},
  // Once per frame: places each voice relative to the listener and measures the level.
  frame(){
   const bank=audio.bank;if(!bank)return;
   const p=bank.listenerPosition,q=bank.listenerRotation,t=now();
   for(const [slot,v] of voices){
    if(!node(slot)?.panner)continue;
    // Listener space: the inverse listener rotation of the offset. The Web Audio listener stays at the origin.
    const x=v.position[0]-p[0],y=v.position[1]-p[1],z=v.position[2]-p[2];
    const qx=-q[0],qy=-q[1],qz=-q[2],qw=q[3];
    const tx=2*(qy*z-qz*y),ty=2*(qz*x-qx*z),tz=2*(qx*y-qy*x);
    relative[0]=x+qw*tx+(qy*tz-qz*ty);relative[1]=y+qw*ty+(qz*tx-qx*tz);relative[2]=z+qw*tz+(qx*ty-qy*tx);
    v.panner.positionX.value=relative[0];v.panner.positionY.value=relative[1];v.panner.positionZ.value=relative[2];
    // The host relays this stream, so a gone figure mutes here and not at the track.
    v.gain.gain.value=v.placed?1:0;
    samples??=new Float32Array(v.analyser.fftSize);
    v.analyser.getFloatTimeDomainData(samples);
    let sum=0;for(let i=0;i<samples.length;i++)sum+=samples[i]*samples[i];
    if(Math.sqrt(sum/samples.length)>SPEAKING_LEVEL)v.heard=t;
   }
  },
  speaking(slot){const v=voices.get(slot);return !!v&&now()-v.heard<SPEAKING_MS;},
  clear(){for(const v of voices.values())drop(v);voices.clear();},
 };
}
