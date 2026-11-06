// The simulation runs in a Web Worker. One command is in flight. The renderer reads each frame once.
export function createSimulation(worker,onFailure){
 let frame=null,sandboxWaits=[],failed=false,starting=null,started=false,netStart=null,tickMs=0,ticks=0,wasmBytes=0,hooks={},netIn=[],onNet=null;
 const fail=message=>{if(failed)return;failed=true;frame=null;starting?.reject(Error(message));starting=null;onFailure(message);};
 worker.addEventListener('message',({data})=>{
  if(failed)return;
  if(data.error!==undefined){fail(data.error);return;}
  if(data.sandbox!==undefined){sandboxWaits.shift()?.(data.sandbox);return;}
  if(data.started){started=true;if(netStart){const [role,slot,handler]=netStart;netStart=null;netSession(role,slot,handler);}starting?.resolve({loadError:data.loadError});starting=null;return;}
  frame=data.frame;tickMs+=data.tickMs;ticks++;wasmBytes=data.wasmBytes;
  if(data.net?.length)onNet?.(data.net);
  if(data.clearInput)hooks.clearInput?.();
  if(data.inputConsumed)hooks.inputConsumed?.();
 });
 // Before the start, the last session waits. Peer packets before it are dropped.
 function netSession(role,slot,handler){
  if(!started){netStart=[role,slot,handler];return;}
  onNet=role?handler:null;netIn=[];worker.postMessage({netSession:{role,slot}});
 }
 worker.addEventListener('error',event=>fail(event.message||'The simulation worker stopped.'));
 worker.addEventListener('messageerror',()=>fail('The simulation worker message could not be read.'));
 return {
  // Hooks of the CPU profile. The worker reports input use with the frame.
  hooks(value){hooks=value;},
  profile(enabled){worker.postMessage({cpuProfile:Boolean(enabled)});},
  // `manifest` moves to the worker, which parses it before `start`.
  load(manifest){worker.postMessage({manifest},[manifest.buffer]);},
  // `save` moves to the worker. It is the save JSON, or empty for a new game.
  // Resolves with why the save did not load. Commands sent before the start wait in the worker queue.
  start(hash,drawDistance,biome,startCar,seed,options,save){
   return new Promise((resolve,reject)=>{starting={resolve,reject};worker.postMessage({start:{hash,drawDistance,biome,startCar,seed,options,save}},[save.buffer]);});
  },
  // The command is a view of WASM memory, so copy it before transfer.
  send(command){
   if(failed)return;const copy=command.slice(),transfer=[copy.buffer];
   let net=null;
   if(netIn.length){net=concat(netIn);netIn=[];transfer.push(net.buffer);}
   worker.postMessage({command:copy,net},transfer);
  },
  // Multiplayer: `role` 0 off, 1 host, 2 guest. `handler` takes the outbound packets of each frame.
  netSession,
  netLeave(slot){if(started)worker.postMessage({netLeave:slot});},
  // A batch of inbound packets, each with a u16 length. It goes with the next command.
  netPackets(batch){if(onNet)netIn.push(batch);},
  // A JSON array of `simulation::sandbox` actions. Resolves with the JSON array of replies.
  // Call it only after `started`: a sandbox action before the start stops the game.
  sandbox(json){return new Promise(resolve=>{sandboxWaits.push(resolve);worker.postMessage({sandbox:json});});},
  get started(){return started;},
  frameLength:()=>frame?frame.length:-1,
  read(out){out.set(frame);frame=null;},
  // Average tick time since the last read, and the worker WASM memory.
  stats(){const value={tickMs:ticks?tickMs/ticks:null,wasmBytes};tickMs=0;ticks=0;return value;},
 };
}

function concat(batches){
 const out=new Uint8Array(batches.reduce((n,b)=>n+b.length,0));
 let at=0;
 for(const b of batches){out.set(b,at);at+=b.length;}
 return out;
}

export function installSimulationBridge(target,simulation){
 target.gameSimSend=simulation.send;
 target.gameSimFrameLength=simulation.frameLength;
 target.gameSimRead=simulation.read;
}
