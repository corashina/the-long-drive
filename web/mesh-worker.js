// Runs one kind of mesh job in its own WASM instance. `?job=terrain`, `road` or `grass` picks it.
// The host owns init, handshake, the job queue, the capacity check and the profile fields.
import init,* as wasm from './pkg/mesh_worker.js';
// Invalid diagnostic clocks must not fail calculation or transport.
const now=()=>{try{return performance.now();}catch{return NaN;}};
self.meshCpuNow=now;
const buffers=(out,names)=>names.map(name=>out[name].buffer);

// A job returns [reply, transfer, profile]. The profile has only its own durations.
function terrain(data){
 if(data.type!=='evaluate')throw Error('Invalid terrain job.');
 const start=now(),surface=wasm.terrain_evaluate(data.input),computed=now();
 let out;
 try{out={type:'result',id:data.id,vertices:surface.vertices(),normals:surface.normals(),tangents:surface.tangents(),uv:surface.uv(),colors:surface.colors(),triangles:surface.triangles()};}finally{surface.free();}
 return [out,buffers(out,['vertices','normals','tangents','uv','colors','triangles']),{computeMs:computed-start,extractionMs:now()-computed}];
}

function road(){
 const sources=new Array(3).fill(null);
 return data=>{
  const start=now();
  if(data.type==='install'){
   const owner=data.identity.owner;if(!Number.isInteger(owner)||owner<0||owner>=3)throw Error('Invalid road source slot.');
   wasm.road_install(owner,data.input);sources[owner]={session:data.identity.session,revision:data.identity.revision};
   return [{type:'installed',identity:data.identity},[],{installWithInputCopyMs:now()-start}];
  }
  if(data.type!=='evaluate')throw Error('Invalid road job.');
  const source=sources[data.source.owner];
  if(!source||source.session!==data.identity.session||source.revision!==data.source.revision)throw Error('Mismatched road source revision.');
  const surface=wasm.road_evaluate(data.source.owner,data.input.subarray(12),Boolean(data.profileCpu)),computed=now();
  let out,profile;
  try{
   const decodeMs=surface.decode_ms(),computeMs=surface.compute_ms();
   out={type:'result',identity:data.identity,origin:surface.origin(),vertices:surface.vertices(),normals:surface.normals(),tangents:surface.tangents(),uv:surface.uv(),triangles:surface.triangles(),submeshes:surface.submeshes()};
   // WASM binding copy and call overhead, without the decode and compute spans.
   profile={inputCopyMs:computed-start-decodeMs-computeMs,decodeMs,computeMs};
  }finally{surface.free();}
  profile.extractionMs=now()-computed;
  return [out,buffers(out,['origin','vertices','normals','tangents','uv','triangles','submeshes']),profile];
 };
}

function grass(){
 const sources=new Map();let owned=null;
 return data=>{
  const id=data.identity,start=now();
  if(data.type==='install'){
   if(sources.has(id.entity))throw Error('Grass dataset is already installed.');
   wasm.grass_install(BigInt(id.entity),data.input);sources.set(id.entity,id);
   return [{type:'installed',identity:id},[],{computeMs:now()-start}];
  }
  if(data.type==='retire'){
   if(owned?.entity===id.entity)throw Error('Grass dataset has an owned plan.');
   wasm.grass_retire(BigInt(id.entity));sources.delete(id.entity);
   return [{type:'retired',identity:id},[],{}];
  }
  if(data.type==='discard'){wasm.grass_discard();owned=null;return [{type:'released',identity:id},[],{}];}
  if(data.type==='plan'){
   const source=sources.get(id.entity);
   if(owned||!source||source.session!==id.session)throw Error('Grass plan has no installed owner.');
   const allocation=wasm.grass_plan(BigInt(id.entity),data.input);let out;
   try{out={type:'planned',identity:id,allocation:[allocation.vertices(),allocation.indices(),allocation.scratch_bytes(),allocation.output_bytes(),allocation.gpu_bytes()]};}finally{allocation.free();}
   owned=id;return [out,[],{computeMs:now()-start}];
  }
  if(data.type!=='evaluate'||!owned||owned.session!==id.session||owned.revision!==id.revision||owned.entity!==id.entity)throw Error('Grass approval has no owned plan.');
  const surface=wasm.grass_evaluate(),computed=now();let out;
  try{out={type:'result',identity:owned,anchors:surface.anchors(),uvs:surface.uvs(),blades:surface.blades(),patches:surface.patches(),indices:surface.indices()};}finally{surface.free();}
  owned=null;
  return [out,buffers(out,['anchors','uvs','blades','patches','indices']),{computeMs:computed-start,extractionMs:now()-computed}];
 };
}

const params=new URL(self.location.href).searchParams,kind=params.get('job');
const run=kind==='terrain'?terrain:kind==='road'?road():kind==='grass'?grass():null;
const startup=params.get('profileCpu')==='1'?now():null;
let memory,initializationMs=null,initializationError=null,queue=Promise.resolve();
// Handle rejection immediately, even if no job has been submitted.
const ready=init().then(exports=>{
 if(!run)throw Error(`Invalid mesh job: ${kind}.`);
 memory=exports.memory;if(startup!==null)initializationMs=now()-startup;
 self.postMessage({type:'initialized',wasmBytes:memory.buffer.byteLength});
}).catch(error=>{initializationError=error?.message??String(error);self.postMessage({type:'fatal',error:initializationError});});
self.addEventListener('message',({data})=>{
 const received=data.profileCpu||data.type==='handshake'?now():null;
 queue=queue.then(async()=>{
  await ready;if(initializationError!==null)throw Error(initializationError);
  if(data.type==='handshake'){self.postMessage({type:'handshake',windowOrigin:data.windowOrigin,windowSent:data.windowSent,workerOrigin:performance.timeOrigin,workerReceived:received,workerSent:now(),initializationMs,wasmBytes:memory.buffer.byteLength});return;}
  const start=data.profileCpu?now():null;
  const [out,transfer,profile]=run(data);
  out.wasmBytes=memory.buffer.byteLength;
  if(out.wasmBytes>data.capacityLimit)throw Error(`${kind[0].toUpperCase()+kind.slice(1)} WASM capacity exceeds admission.`);
  if(data.profileCpu)out.cpuProfile={workerOrigin:performance.timeOrigin,workerReceived:received,workerReadyWaitMs:start-received,...profile,resultPostStart:now()};
  self.postMessage(out,transfer);
 }).catch(error=>self.postMessage({type:'fatal',error:error?.message??String(error)}));
});
