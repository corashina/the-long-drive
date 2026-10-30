import init,{install,evaluate} from './pkg/road_worker.js';
// Invalid diagnostic clocks must not fail calculation or transport.
const now=()=>{try{return performance.now();}catch{return NaN;}};
self.roadCpuNow=now;
const profileStartup=new URL(self.location.href).searchParams.get('profileCpu')==='1';
const startupStart=profileStartup?now():null;
let initializationMs=null,memory,initializationError=null,queue=Promise.resolve();
const sources=new Array(3).fill(null);
// Handle rejection immediately, even if no job has been submitted.
const ready=init().then(exports=>{memory=exports.memory;if(profileStartup)initializationMs=now()-startupStart;self.postMessage({type:'initialized',wasmBytes:memory.buffer.byteLength});}).catch(error=>{initializationError=error?.message??String(error);self.postMessage({type:'fatal',error:initializationError});});
self.addEventListener('message',({data})=>{
 const received=data.profileCpu||data.type==='handshake'?now():null;
 queue=queue.then(async()=>{
  await ready;
  if(initializationError!==null)throw Error(initializationError);
  if(data.type==='handshake'){self.postMessage({type:'handshake',windowOrigin:data.windowOrigin,windowSent:data.windowSent,workerOrigin:performance.timeOrigin,workerReceived:received,workerSent:now(),initializationMs,wasmBytes:memory.buffer.byteLength});return;}
  const start=data.profileCpu?now():null;
  const checkCapacity=()=>{if(memory.buffer.byteLength>data.capacityLimit)throw Error('Road WASM capacity exceeds admission.');};
  if(data.type==='install'){
   const owner=data.identity.owner;if(!Number.isInteger(owner)||owner<0||owner>=3)throw Error('Invalid road source slot.');
   install(owner,data.input);sources[owner]={session:data.identity.session,revision:data.identity.revision};
   checkCapacity();
   const out={type:'installed',identity:data.identity,wasmBytes:memory.buffer.byteLength};
   if(data.profileCpu)try{out.cpuProfile={workerOrigin:performance.timeOrigin,workerReceived:received,workerReadyWaitMs:start-received,installWithInputCopyMs:now()-start,resultPostStart:now()};}catch{}
   self.postMessage(out);return;
  }
  if(data.type!=='evaluate')throw Error('Invalid road worker operation.');
  const source=sources[data.source.owner];
  if(!source||source.session!==data.identity.session||source.revision!==data.source.revision)throw Error('Mismatched road source revision.');
  const surface=evaluate(data.source.owner,data.input.subarray(12),Boolean(data.profileCpu));
  const calculated=data.profileCpu?now():null;
  let out;
  try{
   out={type:'result',identity:data.identity,origin:surface.origin(),vertices:surface.vertices(),normals:surface.normals(),tangents:surface.tangents(),uv:surface.uv(),triangles:surface.triangles(),submeshes:surface.submeshes(),wasmBytes:memory.buffer.byteLength};
   if(data.profileCpu)try{const decodeMs=surface.decode_ms(),computeMs=surface.compute_ms();out.cpuProfile={workerOrigin:performance.timeOrigin,workerReceived:received,workerReadyWaitMs:start-received,inputCopyMs:calculated-start-decodeMs-computeMs,inputCopyBasis:'WASM binding copy and call overhead, excluding decode and calculation spans.',decodeMs,computeMs,extractionMs:now()-calculated,initializationMs,wasmBytes:out.wasmBytes,resultPostStart:now()};}catch{}
  }finally{surface.free();}
  checkCapacity();
  if(data.profileCpu&&out.cpuProfile)try{out.cpuProfile.extractionMs=now()-calculated;out.cpuProfile.resultPostStart=now();}catch{}
  self.postMessage(out,[out.origin.buffer,out.vertices.buffer,out.normals.buffer,out.tangents.buffer,out.uv.buffer,out.triangles.buffer,out.submeshes.buffer]);
 }).catch(error=>self.postMessage({type:'fatal',error:error?.message??String(error)}));
});
