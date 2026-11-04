// The transport of one compute worker: replies, failure, dispose, capacity and the profile clock.
// Road, grass and terrain clients keep only their own job protocol.
import {workerClockHandshake,workerDeliveryInterval,workerInputDeliveryInterval} from './cpu-profile.js';
const MAX_U64=18446744073709551615n;
export const u64=value=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=MAX_U64;
export const metric=value=>({valid:Number.isFinite(value)&&value>=0,valueMs:Number.isFinite(value)&&value>=0?value:null,rawMs:Number.isFinite(value)?value:String(value),reason:!Number.isFinite(value)?'invalid-duration':value<0?'negative-duration':null});
// `receive(data,received)` gets each job reply. `onFail(message)` drops the client state.
// Workers reply `initialized`, `handshake` and `fatal`, and give `wasmBytes` with every other reply.
// `capacity` is the first admitted WASM size. Rust raises it with `capacityLimit`.
export function createWorkerLink(worker,kind,profile,now,{receive,onFail,capacity=2097152}){
 let failure=null,disposed=false,initialized=false,wasmBytes=0,capacityLimit=capacity,clock=null,handshakePending=false;
 const diagnostic=fn=>{if(profile?.enabled){try{fn();}catch{/* Diagnostics never change transport ownership. */}}};
 const emit=record=>diagnostic(()=>profile.worker(record));
 const fail=message=>{if(disposed||failure!==null)return;failure=String(message);worker.terminate();onFail(failure);initialized=false;wasmBytes=0;handshakePending=false;emit({kind,phase:'failure',error:failure});};
 const profileHandshake=()=>diagnostic(()=>{if(disposed||failure!==null||handshakePending)return;worker.postMessage({type:'handshake',windowOrigin:profile.timeOrigin,windowSent:now()});handshakePending=true;});
 worker.addEventListener('message',({data})=>{
  if(disposed||failure!==null)return;
  let received;diagnostic(()=>{received=now();});
  if(!data||typeof data!=='object'){fail(`Malformed ${kind} worker message.`);return;}
  if(data.type==='fatal'){fail(data.error??`The ${kind} worker stopped.`);return;}
  if(data.type==='handshake'){
   handshakePending=false;
   diagnostic(()=>{clock=workerClockHandshake({...data,windowReceived:received});emit({kind,phase:'initialization',clockHandshake:clock,worker:{initializationMs:data.initializationMs??null,retainedWasmBytes:data.wasmBytes??null}});});
   return;
  }
  if(!Number.isSafeInteger(data.wasmBytes)||data.wasmBytes<0||data.wasmBytes>capacityLimit){fail(`${kind[0].toUpperCase()+kind.slice(1)} WASM capacity exceeds admission.`);return;}
  wasmBytes=data.wasmBytes;
  if(data.type==='initialized'){initialized=true;return;}
  receive(data,received);
 });
 worker.addEventListener('error',event=>fail(event.message||`The ${kind} worker stopped.`));
 worker.addEventListener('messageerror',()=>fail(`The ${kind} worker message could not be read.`));
 return {
  diagnostic,emit,fail,profileHandshake,
  handshakePending:()=>handshakePending,
  clock:()=>clock,
  initialized:()=>initialized,
  wasmBytes:()=>wasmBytes,
  live(){if(disposed)throw Error(`The ${kind} worker is disposed.`);if(failure!==null)throw Error(failure);},
  closed:()=>disposed||failure!==null,
  capacityLimit(bytes){if(!Number.isSafeInteger(bytes)||bytes<capacityLimit)throw Error(`Invalid ${kind} capacity admission.`);capacityLimit=bytes;},
  post(message,transfer=[]){try{worker.postMessage({...message,capacityLimit},transfer);}catch(error){fail(error.message);throw error;}},
  // Main thread and worker delivery times of one reply.
  timing:(record,data,received)=>diagnostic(()=>{
   if(!record)return;
   record.resultReceived=received;record.worker=data.cpuProfile??null;
   const raw=data.cpuProfile;
   if(!raw){record.timing={valid:false,reason:'missing-worker-diagnostics'};return;}
   const returned=workerDeliveryInterval(clock,{workerSent:raw.resultPostStart,windowReceived:received});
   const submitted=workerInputDeliveryInterval(clock,{windowSent:record.messageSubmitStart,workerReceived:raw.workerReceived});
   if(raw.workerOrigin!==clock?.workerOrigin)for(const interval of [returned,submitted]){interval.valid=false;interval.valueMs=null;interval.reason='worker-origin-mismatch';}
   record.clockHandshake=clock;record.timing={returnDelivery:returned,submitDelivery:submitted};record.returnDeliveryMs=returned.valueMs;record.submitDeliveryMs=submitted.valueMs;
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
  profileHandshake(){for(const client of clients)client.profileHandshake();},
  profileState(){let jobActive=false,handshakeActive=false,publicationActive=false;for(const client of clients){const s=client.profileState();jobActive||=s.jobActive;handshakeActive||=s.handshakeActive;publicationActive||=Boolean(s.publicationActive);}return {jobActive,handshakeActive,publicationActive};},
 };
}
