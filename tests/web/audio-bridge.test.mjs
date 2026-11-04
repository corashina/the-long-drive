import test from 'node:test';
import assert from 'node:assert/strict';
import {installAudioBridge,OP} from '../../web/audio.js';
test('audio_frame_decodes_every_opcode',()=>{
 const calls=[],audio=new Proxy({},{get:(_,name)=>(...args)=>calls.push([name,...args.map(a=>a instanceof Float32Array?[...a]:typeof a==='object'&&a?structuredClone(a):a)])});
 const host={};installAudioBridge(host,audio);
 const id=0x50000003;
 host.gameAudioFrame(new Float64Array([
  OP.BEGIN,7,1,0,12.5,
  OP.SURVIVAL_MASTER,.5,3,12.5,
  OP.PLAYER_EAR,-6,9,.25,-1,
  OP.ORIGIN_SHIFT,1,2,3,
  OP.LISTENER,1,2,3,0,0,0,1,
  OP.SURVIVAL_EMITTER,4,1,2,3,
  OP.SURVIVAL,4,5,6,7,.5,2,1,2,3,
  OP.EMITTER,id,1,2,3,0,0,0,1,1,
  OP.VOICE,id,1,.5,2,0,8,
  OP.EVENT,7,9,10,id,1,2,3,.5,2,12.5,
  OP.MIXER,2,-3,
  OP.ROUTE,id,2,
  OP.RADIO,id,1,0,98.5,.5,.25,11,1.5,.5,12,0,0,0,0,
  OP.END,
 ]));
 const radio={power:true,am:false,frequency:98.5,volume:.5,staticGain:.25,primary:{clip:11,offsetSeconds:1.5,gain:.5,nextClip:12},secondary:{clip:null,offsetSeconds:0,gain:0,nextClip:null}};
 assert.deepEqual(calls,[
  ['begin',7,true,false,12.5],
  ['survivalMaster',.5,3,12.5],
  ['playerEar',-6,9,.25,-1],
  ['originShift',1,2,3],
  ['listener',[1,2,3],[0,0,0,1]],
  ['survivalEmitter',4,1,2,3],
  ['survival',4,5,6,7,.5,2,1,2,3],
  ['emitter',id,[1,2,3],[0,0,0,1],true],
  ['voice',id,true,.5,2,false,8],
  ['event',7,9,10,id,1,2,3,.5,2,12.5],
  ['mixer',2,-3],
  ['route',id,2],
  ['radio',id,radio],
  ['finish'],
 ]);
});
