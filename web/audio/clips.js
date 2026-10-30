import {readPackageAsset} from '../world-assets.js';
// Accepted WAV bytes are checked before browser decoding.
export class ClipCache {
 constructor(context,raw,current,fail){this.context=context;this.clips=new Map(raw.clips.map(c=>[c.id,c]));this.current=current;this.fail=fail;this.buffers=new Map();this.pending=new Map();this.failed=new Set();this.controllers=new Set();this.generation=0;this.radioBuffers=new Map();this.radioPending=new Map();this.radioWanted=new Set();this.radioDemand=[];this.radioDirty=true;}
 async effect(id,session){
  if(this.failed.has(id))return null;
  if(this.buffers.has(id))return this.buffers.get(id);
  if(this.pending.has(id))return this.pending.get(id);
  const generation=this.generation,abort=new AbortController();this.controllers.add(abort);
  const task=(async()=>{try{
   const buffer=await this.decode(id,session,'effect',generation,abort);
   if(buffer&&generation===this.generation&&session===this.current())this.buffers.set(id,buffer);return buffer;
  }catch(error){if(generation===this.generation&&session===this.current()&&error.name!=='AbortError'){this.failed.add(id);this.fail(id,error);}return null;}
  finally{this.controllers.delete(abort);if(generation===this.generation)this.pending.delete(id);}})();
  this.pending.set(id,task);return task;
 }
 async decode(id,session,usage,generation,abort){
   const clip=this.clips.get(id);if(!clip||!clip.usages.includes(usage))throw Error('Invalid '+usage+' clip');
   const input=await readPackageAsset(clip.path,abort.signal),bytes=input.buffer;
   if(generation!==this.generation||session!==this.current())return null;
   if(bytes.byteLength!==clip.bytes)throw Error('Audio byte count');
   const view=new DataView(bytes);if(view.getUint32(0,true)!==0x46464952||view.getUint32(8,true)!==0x45564157)throw Error('Audio WAV header');
   let format=false,data=false;for(let offset=12;offset+8<=bytes.byteLength;){const size=view.getUint32(offset+4,true),end=offset+8+size;if(end>bytes.byteLength)throw Error('Audio WAV chunk');if(view.getUint32(offset,true)===0x20746d66){if(size<16||view.getUint16(offset+8,true)!==clip.format_tag||view.getUint16(offset+10,true)!==clip.channels||view.getUint32(offset+12,true)!==clip.frequency||view.getUint16(offset+22,true)!==clip.bits_per_sample)throw Error('Audio WAV format');format=true;}if(view.getUint32(offset,true)===0x61746164){if(size!==clip.data_bytes)throw Error('Audio WAV samples');data=true;}offset=end+(size&1);}
   if(!format||!data)throw Error('Audio WAV data');
   const buffer=await this.context.decodeAudioData(bytes);
   if(generation!==this.generation||session!==this.current())return null;
   if(Math.abs(buffer.duration-clip.duration_seconds)>.02||buffer.numberOfChannels!==clip.channels)throw Error('Audio decoded samples');
   return buffer;
 }
 radio(demand,session){
  if(session!==this.current())return;
  const last=this.radioDemand;let changed=last.length!==demand.length;
  for(let i=0;!changed&&i<demand.length;i++)changed=last[i]!==demand[i];
  if(!changed&&!this.radioDirty)return;this.radioDirty=false;
  if(changed){if(last.length!==demand.length)this.radioDemand=demand.slice();else for(let i=0;i<demand.length;i++)last[i]=demand[i];this.radioWanted.clear();for(const id of demand)if(id!==null)this.radioWanted.add(id);}
  for(const id of this.radioBuffers.keys())if(!this.radioWanted.has(id))this.radioBuffers.delete(id);
  for(const entry of this.radioPending.values())if(!this.radioWanted.has(entry.id)){entry.stale=true;entry.abort.abort();}
  for(const id of this.radioWanted){
   if(this.failed.has(id)||this.radioBuffers.has(id)||this.radioPending.has(id))continue;
   if(this.radioBuffers.size+this.radioPending.size>=4){
    // A full cache gives way only to a clip earlier in demand order.
    let victim=null,worst=demand.indexOf(id);
    for(const other of this.radioBuffers.keys()){const rank=demand.indexOf(other);if(rank>worst){worst=rank;victim=other;}}
    for(const other of this.radioPending.values()){const rank=demand.indexOf(other.id);if(rank>worst){worst=rank;victim=other;}}
    if(victim===null)break;
    if(typeof victim==='object'){victim.stale=true;victim.abort.abort();this.radioPending.delete(victim.id);}else this.radioBuffers.delete(victim);
   }
   const generation=this.generation,abort=new AbortController(),entry={id,abort,stale:false,task:null};
   this.radioPending.set(id,entry);
   entry.task=this.decode(id,session,'radio',generation,abort).then(buffer=>{
    if(buffer&&!entry.stale&&generation===this.generation&&session===this.current()&&this.radioWanted.has(id)){this.radioPending.delete(id);this.radioBuffers.set(id,buffer);}
   }).catch(error=>{if(!entry.stale&&generation===this.generation&&session===this.current()&&error.name!=='AbortError'){this.failed.add(id);this.fail(id,error);}}).finally(()=>{if(this.radioPending.get(id)===entry)this.radioPending.delete(id);this.radioDirty=true;});
  }
 }
 reset(){this.generation++;for(const controller of this.controllers)controller.abort();this.controllers.clear();this.pending.clear();this.failed.clear();this.radioWanted.clear();this.radioDemand.fill(null);this.radioDirty=true;this.radioBuffers.clear();for(const entry of this.radioPending.values()){entry.stale=true;entry.abort.abort();}}
 dispose(){this.reset();this.buffers.clear();}
}
