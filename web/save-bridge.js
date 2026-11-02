export function createSaveBridge(host=window){
 let sequence=0;
 const queue=[],pending=new Map(),committing=new Set();
 function request(kind,payload,onDispatch,signal){
  const id=++sequence;
  return new Promise((resolve,reject)=>{
   const cancel=()=>{
    if(!pending.has(id))return;
    pending.delete(id);
    const index=queue.findIndex(item=>item.id===id);
    if(index!==-1)queue.splice(index,1);
    else if(kind==='PrepareNewGame'){
     request('Discard',id).catch(()=>{});queue.unshift(queue.pop());
    }
    reject(new DOMException('Save operation canceled.','AbortError'));
   };
   if(signal?.aborted){reject(new DOMException('Save operation canceled.','AbortError'));return;}
   pending.set(id,{kind,resolve,reject,onDispatch,signal,cancel});queue.push({id,kind,payload});signal?.addEventListener('abort',cancel,{once:true});
  });
 }
 host.gameHasSaveRequest=()=>queue.length!==0;
 host.gameSaveRequest=()=>{
  if(!queue.length)return '';
  const request=queue.shift();
  // Rust takes and processes the request synchronously, without yielding to JavaScript.
  if(request.kind==='Commit'){committing.add(request.payload);pending.get(request.id).onDispatch?.();}
  return JSON.stringify(request);
 };
 host.gameSaveResult=(id,kind,payload,error)=>{
  const operation=pending.get(id);
  if(!operation||operation.kind!==kind)return;
  pending.delete(id);
  operation.signal?.removeEventListener('abort',operation.cancel);
  if(error){operation.reject(new Error(error));return;}
  try{operation.resolve(payload?JSON.parse(payload):undefined);}catch(error){operation.reject(error);}
 };
 return {
  capture:(autosave=false)=>request('Capture',{date:new Date().toISOString(),autosave}),
  prepareNewGame:(signal,drawDistance)=>request('PrepareNewGame',{draw_distance:drawDistance},undefined,signal),
  commit(token,onDispatch){return request('Commit',token,onDispatch).finally(()=>committing.delete(token));},
  discard(token){
   if(committing.has(token))return;
   const index=queue.findIndex(request=>request.kind==='Commit'&&request.payload===token);
   if(index!==-1){
    const [request]=queue.splice(index,1),operation=pending.get(request.id);pending.delete(request.id);
    operation.reject(new DOMException('Save operation canceled.','AbortError'));
   }
   const result=request('Discard',token);queue.unshift(queue.pop());
   // Discard is cleanup. It never changes the active session.
   result.catch(()=>{});
  }
 };
}
