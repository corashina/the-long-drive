import init,{evaluate} from './pkg/terrain_worker.js';
const now=()=>{try{return performance.now();}catch{return NaN;}};
const startup=new URL(self.location.href).searchParams.get('profileCpu')==='1'?now():null;
let initializationMs=null,memory,initializationError=null,queue=Promise.resolve();
const ready=init().then(exports=>{memory=exports.memory;if(startup!==null)initializationMs=now()-startup;self.postMessage({type:'initialized',initializationMs,wasmBytes:memory.buffer.byteLength});}).catch(error=>{initializationError=error?.message??String(error);self.postMessage({type:'fatal',error:initializationError});});
self.addEventListener('message',({data})=>{
 const received=data.profileCpu?now():null;
 queue=queue.then(async()=>{
  await ready;if(initializationError!==null)throw Error(initializationError);
  if(data.type!=='evaluate')throw Error('Invalid terrain worker operation.');
  const start=data.profileCpu?now():0;
  const surface=evaluate(data.input);
  const calculated=data.profileCpu?now():0;
  let out;
  try{out={type:'result',id:data.id,vertices:surface.vertices(),normals:surface.normals(),tangents:surface.tangents(),uv:surface.uv(),colors:surface.colors(),triangles:surface.triangles(),wasmBytes:memory.buffer.byteLength};}finally{surface.free();}
  if(out.wasmBytes>data.capacityLimit)throw Error('Terrain WASM capacity exceeds admission.');
  if(data.profileCpu)out.cpuProfile={workerReadyWaitMs:start-received,computeMs:calculated-start,extractionMs:now()-calculated};
  self.postMessage(out,[out.vertices,out.normals,out.tangents,out.uv,out.colors,out.triangles].map(a=>a.buffer));
 }).catch(error=>self.postMessage({type:'fatal',error:error?.message??String(error)}));
});
