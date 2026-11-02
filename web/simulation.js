// The simulation runs in a Web Worker. One command is in flight; the renderer reads each frame once.
export function createSimulation(worker,onFailure){
 let frame=null,failed=false,starting=null,started=false,netStart=null,tickMs=0,ticks=0,wasmBytes=0,hooks={},netIn=[],onNet=null;
 const fail=message=>{if(failed)return;failed=true;frame=null;starting?.reject(Error(message));starting=null;onFailure(message);};
 worker.addEventListener('message',({data})=>{
  if(failed)return;
  if(data.error!==undefined){fail(data.error);return;}
  if(data.startup){started=true;if(netStart){const [role,slot,handler]=netStart;netStart=null;netSession(role,slot,handler);}starting?.resolve({startup:data.startup,loadError:data.loadError});starting=null;return;}
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
  // Hooks of the CPU profile: the worker reports input use with the frame.
  hooks(value){hooks=value;},
  profile(enabled){worker.postMessage({cpuProfile:Boolean(enabled)});},
  // `manifest` and `save` move to the worker. `save` is the save JSON, or empty for a new game.
  // Resolves with the encoded startup values and why the save did not load.
  start(manifest,hash,drawDistance,biome,startCar,seed,options,save){
   return new Promise((resolve,reject)=>{starting={resolve,reject};worker.postMessage({start:{manifest,hash,drawDistance,biome,startCar,seed,options,save}},[manifest.buffer,save.buffer]);});
  },
  // The command is a view of WASM memory, so copy it before transfer.
  send(command){
   if(failed)return;const copy=command.slice(),transfer=[copy.buffer];
   let net=null;
   if(netIn.length){net=joinPackets(netIn);netIn=[];transfer.push(net.buffer);}
   worker.postMessage({command:copy,net},transfer);
  },
  // Multiplayer: `role` 0 off, 1 host, 2 guest. `handler` takes the outbound packets of each frame.
  netSession,
  netLeave(slot){if(started)worker.postMessage({netLeave:slot});},
  // One inbound packet. It rides with the next command.
  netPacket(packet){if(onNet)netIn.push(packet);},
  frameLength:()=>frame?frame.length:-1,
  read(out){out.set(frame);frame=null;},
  // Average tick time since the last read, and the worker WASM memory.
  stats(){const value={tickMs:ticks?tickMs/ticks:null,wasmBytes};tickMs=0;ticks=0;return value;},
 };
}

// Packets with a u16 length each, as `simulation::net` reads them.
export function joinPackets(packets){
 const out=new Uint8Array(packets.reduce((n,p)=>n+2+p.length,0));
 let at=0;
 for(const p of packets){out[at]=p.length&255;out[at+1]=p.length>>8;out.set(p,at+2);at+=2+p.length;}
 return out;
}
export function splitPackets(bytes){
 const packets=[];
 for(let at=0;at+2<=bytes.length;){const n=bytes[at]|bytes[at+1]<<8;packets.push(bytes.subarray(at+2,at+2+n));at+=2+n;}
 return packets;
}

export function installSimulationBridge(target,simulation){
 target.gameSimSend=simulation.send;
 target.gameSimFrameLength=simulation.frameLength;
 target.gameSimRead=simulation.read;
}
