// Runs the simulation WASM. The terrain and road workers start here, next to the simulation that uses them.
import init,{start,load_error,tick,cpu_profile,net_session,net_receive,net_leave,net_send} from './pkg/simulation_worker.js';
import {createTerrain,installTerrainBridge} from './terrain.js';
import {installRoadBridge} from './road.js';
self.gameNow=()=>performance.now();
let inputConsumed=false,clearInput=false,memory,netOn=false;
self.gameCpuInputConsumed=()=>{inputConsumed=true;};
self.gameCpuClearInput=()=>{clearInput=true;};
installTerrainBridge(self,createTerrain(new Worker(new URL('./terrain-worker.js',import.meta.url),{type:'module'}),null));
installRoadBridge(self,null);
const ready=init().then(exports=>{memory=exports.memory;});
self.addEventListener('message',async({data})=>{
 try{
  await ready;
  if(data.cpuProfile!==undefined){cpu_profile(data.cpuProfile);return;}
  if(data.start){
   const {manifest,hash,drawDistance,biome,startCar,seed,options,save}=data.start;
   const startup=start(manifest,hash,drawDistance,biome,startCar,seed,options,save);
   self.postMessage({startup,loadError:load_error()},[startup.buffer]);return;
  }
  // Multiplayer: `net.js` owns the peers. Packets ride with the commands and frames.
  if(data.netSession){const {role,slot}=data.netSession;net_session(role,slot);netOn=role!==0;return;}
  if(data.netLeave!==undefined){net_leave(data.netLeave);return;}
  if(data.net)net_receive(data.net);
  const started=performance.now(),frame=tick(data.command),tickMs=performance.now()-started;
  const net=netOn?net_send():null,transfer=[frame.buffer];
  if(net)transfer.push(net.buffer);
  self.postMessage({frame,tickMs,wasmBytes:memory.buffer.byteLength,inputConsumed,clearInput,net},transfer);
  inputConsumed=clearInput=false;
 }catch(error){self.postMessage({error:error?.message??String(error)});}
});
