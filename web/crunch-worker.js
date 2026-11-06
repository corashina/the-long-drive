// Unpacks stored crunch files to BC blocks. One job at a time, in message order.
import init,{unpack} from './pkg/crunch_worker.js';
const ready=init();
self.addEventListener('message',async({data:{id,file,first}})=>{
 try{
  await ready;
  const blocks=unpack(file,first);
  self.postMessage({id,blocks},[blocks.buffer]);
 }catch(error){self.postMessage({id,error:error?.message??String(error)});}
});
