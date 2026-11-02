import init,{evaluate} from './pkg/terrain_worker.js';
const profileStartup=new URL(self.location.href).searchParams.get('profileCpu')==='1';
const startupStarted=profileStartup?performance.now():null;
let initializationMs=null,memory;
const ready=init().then(exports=>{memory=exports.memory;if(profileStartup)initializationMs=performance.now()-startupStarted;});
self.addEventListener('message',async({data})=>{
 try{
  if(data.cpuHandshake){const workerReceived=performance.now();await ready;self.postMessage({cpuHandshake:{...data.cpuHandshake,workerOrigin:performance.timeOrigin,workerReceived,workerSent:performance.now(),initializationMs,wasmBytes:memory?.buffer.byteLength??null}});return;}
  const received=data.profileCpu?performance.now():0;
  await ready;
  const evaluateStart=data.profileCpu?performance.now():0;
  const surface=evaluate(data.input);
  const evaluateEnd=data.profileCpu?performance.now():0;
  const out={id:data.id,vertices:surface.vertices(),normals:surface.normals(),tangents:surface.tangents(),
   uv:surface.uv(),colors:surface.colors(),triangles:surface.triangles()};
  surface.free();
  if(data.profileCpu)out.cpuProfile={workerOrigin:performance.timeOrigin,workerReceived:received,workerReadyWaitMs:evaluateStart-received,evaluateWithInputCopyMs:evaluateEnd-evaluateStart,extractionMs:performance.now()-evaluateEnd,initializationMs,wasmBytes:memory?.buffer.byteLength??null,outputBytes:out.vertices.byteLength+out.normals.byteLength+out.tangents.byteLength+out.uv.byteLength+out.colors.byteLength+out.triangles.byteLength,resultPostStart:performance.now()};
  self.postMessage(out,[out.vertices,out.normals,out.tangents,out.uv,out.colors,out.triangles].map(a=>a.buffer));
 }catch(error){self.postMessage({id:data.id,error:error?.message??String(error)});}
});
