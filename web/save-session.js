import {verifyRecord} from './save-storage.js';
const two=value=>String(value).padStart(2,'0');
function formatDate(date){return `${date.getFullYear()}.${two(date.getMonth()+1)}.${two(date.getDate())}. ${two(date.getHours())}:${two(date.getMinutes())}`;}
export function lastSave({saves,autosave,last},buildId){
 const entry=[...saves,autosave].find(entry=>entry&&entry.name===last);
 return entry&&entry.version===buildId?last:null;
}
export function canLoadLast(list,buildId){return lastSave(list,buildId)!==null;}
// The save of the page start: a Load reload, else with "Load last save at start" the last save, except after New game.
export async function startSave({loadName,newGame,autoLoad,list,buildId}){
 if(loadName!==null)return loadName;
 return !newGame&&autoLoad?lastSave(await list(),buildId):null;
}

// `reload` starts the page again from a stored save. It does not resolve.
export function createSaveSession({storage,runtime,drawDistance,reload,onStatus=()=>{}}){
 let sequence=0,active=null,writes=Promise.resolve();
 const current=operation=>active===operation&&operation.id===sequence;
 async function run(name,work){
  if(active)throw new Error('A save operation is pending.');
  const operation={id:++sequence,name,controller:new AbortController(),candidate:null,committing:false};
  active=operation;
  let failed=false,failure;
  try{
   onStatus({operation:name,state:'pending'});
   await work(operation);
  }catch(error){
   if(operation.candidate!==null){runtime.discard(operation.candidate);operation.candidate=null;}
   if(current(operation)){failed=true;failure=error;}
  }finally{
   if(current(operation)){
    active=null;
    onStatus(failed?{operation:name,state:'error',error:failure}:{operation:name,state:'success'});
   }
  }
  if(failed)throw failure;
 }
 async function prepare(operation,build){
  const token=await build();
  if(!current(operation)){runtime.discard(token);return;}
  operation.candidate=token;
  await runtime.commit(token,()=>{operation.committing=true;});
  operation.candidate=null;
 }
 const load=name=>run('load',async operation=>{
  const record=await storage.read(name);
  if(!current(operation))return;
  if(record===null)throw new Error('No saved game.');
  await verifyRecord(record);
  if(current(operation))await reload(name);
 });
 return {
  get pending(){return active!==null;},
  list(){return storage.list();},
  save(name,thumbnail,{autosave=false}={}){return run('save',async operation=>{
   const {save,summary}=await runtime.capture(autosave);
   if(!current(operation))return;
   const entry={name,date:formatDate(new Date()),seed:summary.seed,drivenKm:summary.driven_km,version:summary.build_id,thumbnail};
   // Preserve ordering even when an adapter ignores cancellation during a delayed write.
   const write=writes.then(async()=>{
    if(current(operation))await storage.write(name,save,entry,{signal:operation.controller.signal});
   });
   writes=write.catch(()=>{});
   await write;
  });},
  load,
  // Multiplayer host: the world for a joining guest, as JSON. Nothing is stored.
  async share(){const {save}=await runtime.capture(true);return JSON.stringify(save);},
  remove(name){return run('remove',async()=>{await storage.remove(name);});},
  async loadLast(buildId){
   const name=lastSave(await storage.list(),buildId);
   if(name===null)throw new Error('No saved game.');
   return load(name);
  },
  newGame(){return run('newGame',operation=>prepare(operation,()=>runtime.prepareNewGame(operation.controller.signal,drawDistance())));},
  cancel(){
   if(!active||active.committing)return false;
   const operation=active;active=null;sequence++;
   operation.controller.abort();
   if(operation.candidate!==null){runtime.discard(operation.candidate);operation.candidate=null;}
   onStatus({operation:operation.name,state:'canceled'});
   return true;
  }
 };
}
