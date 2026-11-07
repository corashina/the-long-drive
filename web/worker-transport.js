// The transport of one compute worker: replies, failure and dispose.
// Road, grass and terrain clients keep only their own job protocol.
const MAX_U64=18446744073709551615n;
export const u64=value=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=MAX_U64;
export const metric=value=>({valid:Number.isFinite(value)&&value>=0,valueMs:Number.isFinite(value)&&value>=0?value:null,rawMs:Number.isFinite(value)?value:String(value),reason:!Number.isFinite(value)?'invalid-duration':value<0?'negative-duration':null});
// `receive(data,received)` gets each job reply. `onFail(message)` drops the client state.
// Workers reply `initialized` and `fatal`, and give `wasmBytes` with every other reply.
export function createWorkerLink(worker,kind,profile,now,{receive,onFail}){
 let failure=null,disposed=false,initialized=false,wasmBytes=0;
 const diagnostic=fn=>{if(profile?.enabled){try{fn();}catch{/* Diagnostics never change transport ownership. */}}};
 const emit=record=>diagnostic(()=>profile.worker(record));
 const fail=message=>{if(disposed||failure!==null)return;failure=String(message);worker.terminate();onFail(failure);initialized=false;wasmBytes=0;emit({kind,phase:'failure',error:failure});};
 worker.addEventListener('message',({data})=>{
  if(disposed||failure!==null)return;
  let received;diagnostic(()=>{received=now();});
  if(!data||typeof data!=='object'){fail(`Malformed ${kind} worker message.`);return;}
  if(data.type==='fatal'){fail(data.error??`The ${kind} worker stopped.`);return;}
  if(!Number.isSafeInteger(data.wasmBytes)||data.wasmBytes<0){fail(`Malformed ${kind} worker message.`);return;}
  wasmBytes=data.wasmBytes;
  if(data.type==='initialized'){initialized=true;emit({kind,phase:'initialization',worker:{initializationMs:data.initializationMs??null,retainedWasmBytes:wasmBytes}});return;}
  receive(data,received);
 });
 worker.addEventListener('error',event=>fail(event.message||`The ${kind} worker stopped.`));
 worker.addEventListener('messageerror',()=>fail(`The ${kind} worker message could not be read.`));
 return {
  diagnostic,emit,fail,
  initialized:()=>initialized,
  wasmBytes:()=>wasmBytes,
  live(){if(disposed)throw Error(`The ${kind} worker is disposed.`);if(failure!==null)throw Error(failure);},
  closed:()=>disposed||failure!==null,
  post(message,transfer=[]){try{worker.postMessage(message,transfer);}catch(error){fail(error.message);throw error;}},
  // Worker durations of one reply. Each context measures only its own clock.
  timing:(record,data,received)=>diagnostic(()=>{
   if(!record)return;
   record.resultReceived=received;record.worker=data.cpuProfile??null;
   const raw=data.cpuProfile;
   if(!raw){record.timing={valid:false,reason:'missing-worker-diagnostics'};return;}
   record.timing={};
   for(const key of ['workerReadyWaitMs','inputCopyMs','decodeMs','computeMs','extractionMs','installWithInputCopyMs'])if(key in raw)record.timing[key]=metric(raw[key]);
  }),
  dispose(){if(disposed)return false;worker.terminate();disposed=true;return true;},
 };
}
// `target[name]()` creates a client on a new worker at `url`. The result follows the live clients.
export function installWorkerClients(target,name,url,create,profile){
 const clients=new Set();
 target[name]=()=>{
  const workerUrl=new URL(url);if(profile?.enabled)workerUrl.searchParams.set('profileCpu','1');
  const client=create(new Worker(workerUrl,{type:'module'}),profile);clients.add(client);
  const dispose=client.dispose;client.dispose=()=>{dispose();clients.delete(client);};return client;
 };
 return {
  // WASM memory of the live clients.
  wasmBytes(){let total=0;for(const client of clients)total+=client.wasmBytes();return total;},
  profileState(){let jobActive=false,publicationActive=false;for(const client of clients){const s=client.profileState();jobActive||=s.jobActive;publicationActive||=Boolean(s.publicationActive);}return {jobActive,publicationActive};},
 };
}
