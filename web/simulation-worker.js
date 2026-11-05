// Runs the simulation WASM. The terrain and road workers start here, next to the simulation that uses them.
import init,{load,start,load_error,tick,cpu_profile,sandbox,net_session,net_receive,net_leave,net_send} from './pkg/simulation_worker.js';
import {createTerrain,installTerrainBridge} from './terrain.js';
import {installRoadBridge} from './road.js';
self.gameNow=()=>performance.now();
let inputConsumed=false,clearInput=false,memory,net=null;
self.gameCpuInputConsumed=()=>{inputConsumed=true;};
self.gameCpuClearInput=()=>{clearInput=true;};
// A step that holds for 2 s writes why. Copy the line from the console.
self.gameHoldReport=report=>console.warn('[hold] '+report);
installTerrainBridge(self,createTerrain(new Worker(new URL('./terrain-worker.js',import.meta.url),{type:'module'}),null));
installRoadBridge(self,null);
const ready=init().then(exports=>{memory=exports.memory;});
self.addEventListener('message',async({data})=>{
 try{
  await ready;
  if(data.cpuProfile!==undefined){cpu_profile(data.cpuProfile);return;}
  if(data.manifest){load(data.manifest);return;}
  if(data.start){
   const {hash,drawDistance,biome,startCar,seed,options,save}=data.start;
   start(hash,drawDistance,biome,startCar,seed,options,save);
   // A Join start replaces the world. The new world keeps the multiplayer session.
   if(net)net_session(net.role,net.slot);
   self.postMessage({started:true,loadError:load_error()});return;
  }
  // Multiplayer: `net.js` owns the peers. Packets go with the commands and frames.
  if(data.netSession){const {role,slot}=data.netSession;net_session(role,slot);net=role!==0?{role,slot}:null;return;}
  if(data.netLeave!==undefined){net_leave(data.netLeave);return;}
  // `?sandbox` pages only. It runs between ticks, so the next frame shows the result.
  if(data.sandbox!==undefined){self.postMessage({sandbox:sandbox(data.sandbox)});return;}
  if(data.net)net_receive(data.net);
  const started=performance.now(),frame=tick(data.command),tickMs=performance.now()-started;
  const packets=net?net_send():null,transfer=[frame.buffer];
  if(packets)transfer.push(packets.buffer);
  self.postMessage({frame,tickMs,wasmBytes:memory.buffer.byteLength,inputConsumed,clearInput,net:packets},transfer);
  inputConsumed=clearInput=false;
 }catch(error){self.postMessage({error:error?.message??String(error)});}
});
