import init,{install,plan,evaluate,discard,retire} from './pkg/grass_worker.js';
const now=()=>{try{return performance.now();}catch{return NaN;}};
const startup=new URL(self.location.href).searchParams.get('profileCpu')==='1'?now():null;
let memory,initializationMs=null,initializationError=null,queue=Promise.resolve(),owned=null;
const sources=new Map();
const ready=init().then(exports=>{memory=exports.memory;if(startup!==null)initializationMs=now()-startup;self.postMessage({type:'initialized',wasmBytes:memory.buffer.byteLength});}).catch(error=>{initializationError=error?.message??String(error);self.postMessage({type:'fatal',error:initializationError});});
self.addEventListener('message',({data})=>{
 const received=data.profileCpu||data.type==='handshake'?now():null;
 queue=queue.then(async()=>{
  await ready;if(initializationError!==null)throw Error(initializationError);
  if(data.type==='handshake'){self.postMessage({type:'handshake',windowOrigin:data.windowOrigin,windowSent:data.windowSent,workerOrigin:performance.timeOrigin,workerReceived:received,workerSent:now(),initializationMs,wasmBytes:memory.buffer.byteLength});return;}
  const start=data.profileCpu?now():null;
  const reply=out=>{out.wasmBytes=memory.buffer.byteLength;if(out.wasmBytes>data.capacityLimit)throw Error('Grass WASM capacity exceeds admission.');if(data.profileCpu)out.cpuProfile={workerOrigin:performance.timeOrigin,workerReceived:received,workerReadyWaitMs:start-received,operationMs:now()-start,resultPostStart:now()};return out;};
  if(data.type==='install'){
   if(sources.has(data.identity.entity))throw Error('Grass dataset is already installed.');
   install(BigInt(data.identity.entity),data.input);sources.set(data.identity.entity,data.identity);
   self.postMessage(reply({type:'installed',identity:data.identity}));return;
  }
  if(data.type==='retire'){
   if(owned?.entity===data.identity.entity)throw Error('Grass dataset has an owned plan.');
   retire(BigInt(data.identity.entity));sources.delete(data.identity.entity);self.postMessage(reply({type:'retired',identity:data.identity}));return;
  }
  if(data.type==='discard'){
   discard();owned=null;self.postMessage(reply({type:'released',identity:data.identity}));return;
  }
  if(data.type==='plan'){
   const source=sources.get(data.identity.entity);
   if(owned||!source||source.session!==data.identity.session)throw Error('Grass plan has no installed owner.');
   const allocation=plan(BigInt(data.identity.entity),data.input);
   let out;try{out=reply({type:'planned',identity:data.identity,allocation:[allocation.vertices(),allocation.indices(),allocation.scratch_bytes(),allocation.output_bytes(),allocation.gpu_bytes()]});}finally{allocation.free();}
   if(data.profileCpu&&out.cpuProfile)out.cpuProfile.planningWithInputCopyMs=out.cpuProfile.operationMs;
   owned=data.identity;self.postMessage(out);return;
  }
  if(data.type!=='evaluate'||!owned||owned.session!==data.identity.session||owned.revision!==data.identity.revision||owned.entity!==data.identity.entity)throw Error('Grass approval has no owned plan.');
  const surface=evaluate();const calculated=data.profileCpu?now():null;let out;
  try{out=reply({type:'result',identity:owned,anchors:surface.anchors(),uvs:surface.uvs(),blades:surface.blades(),patches:surface.patches(),indices:surface.indices()});}finally{surface.free();}
  if(data.profileCpu&&out.cpuProfile){out.cpuProfile.emissionWithCallMs=calculated-start;out.cpuProfile.extractionMs=now()-calculated;out.cpuProfile.resultPostStart=now();}
  owned=null;self.postMessage(out,[out.anchors.buffer,out.uvs.buffer,out.blades.buffer,out.patches.buffer,out.indices.buffer]);
 }).catch(error=>self.postMessage({type:'fatal',error:error?.message??String(error)}));
});
