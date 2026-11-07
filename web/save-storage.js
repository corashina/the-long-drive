import {gzip,gunzip} from './gzip.js';
// A save record is the gzip of the Rust save JSON bytes. Older databases are dropped.
const DATABASE='tld-browser-save',VERSION=3,SAVES='saves',INDEX='index',META='meta',LAST='last',STORES=[SAVES,INDEX,META];
export const AUTOSAVE='autosave';
function aborted(){return new DOMException('Save operation canceled.','AbortError');}
function checkSignal(signal){if(signal?.aborted)throw aborted();}
// Mirror TrimSaveName: keep letters, digits, punctuation, separators and symbols. Drop control characters.
export function trimName(text){return String(text).replace(/\p{Cc}|[^\p{L}\p{N}\p{P}\p{Z}\p{S}]/gu,'');}
export function defaultName(date){
 const two=value=>String(value).padStart(2,'0');
 return `${two(date.getFullYear()%100)}_${two(date.getMonth()+1)}_${two(date.getDate())}_${two(date.getHours())}${two(date.getMinutes())}`;
}
// The export names its runtime package by the SHA-256 of the manifest (tools/reference/runtime_package.py `package_id`).
// Saves and multiplayer use that hash as the manifest stamp, so the page does not hash the 100 MB manifest again.
export function manifestStamp(reference){return reference.content_base.slice('assets/runtime/'.length,-1);}
const byName=(a,b)=>a.name<b.name?-1:a.name>b.name?1:0;
export function createSaveStorage({indexedDB=globalThis.indexedDB}={}){
 let connection=null,opening=null,closed=false;
 const transactions=new Set();
 function open(){
  if(closed)return Promise.reject(new Error('Save storage is closed.'));
  if(!indexedDB)return Promise.reject(new Error('Save storage is unavailable.'));
  if(connection)return Promise.resolve(connection);
  if(opening)return opening.promise;
  const state={settled:false};
  state.promise=new Promise((resolve,reject)=>{
   const fail=error=>{if(state.settled)return;state.settled=true;if(opening===state)opening=null;reject(error);};
   state.fail=fail;
   let request;
   try{request=indexedDB.open(DATABASE,VERSION);}catch(error){fail(error);return;}
   request.onblocked=()=>fail(new Error('Save database access is blocked. Close the other tab.'));
   request.onerror=()=>fail(request.error??new Error('Cannot open the save database.'));
   request.onupgradeneeded=event=>{
    if(state.settled||closed){request.transaction.abort();return;}
    if(event.oldVersion>VERSION){request.transaction.abort();fail(new Error('Incompatible save database version.'));return;}
    const db=request.result;
    for(const name of [...db.objectStoreNames])db.deleteObjectStore(name);
    for(const name of STORES)db.createObjectStore(name);
   };
   request.onsuccess=()=>{
    const db=request.result;
    if(state.settled||closed){db.close();return;}
    if(db.version!==VERSION||!STORES.every(name=>db.objectStoreNames.contains(name))){db.close();fail(new Error('Incompatible save database version or store.'));return;}
    state.settled=true;opening=null;connection=db;
    db.onversionchange=()=>{db.close();if(connection===db)connection=null;};
    resolve(db);
   };
  });
  if(!state.settled)opening=state;
  return state.promise;
 }
 // work(stores,track) issues the requests and returns a function that gives the result after completion.
 async function transaction(names,mode,work,signal){
  checkSignal(signal);
  const db=await open();
  checkSignal(signal);
  return new Promise((resolve,reject)=>{
   let tx,result=()=>undefined,requestError;
   try{tx=mode==='readwrite'?db.transaction(names,mode,{durability:'strict'}):db.transaction(names,mode);}catch(error){reject(error);return;}
   transactions.add(tx);
   const cancel=()=>{try{tx.abort();}catch{/* A completed transaction stays committed. */}};
   const cleanup=()=>{transactions.delete(tx);signal?.removeEventListener('abort',cancel);};
   tx.oncomplete=()=>{cleanup();resolve(result());};
   tx.onabort=()=>{cleanup();reject(tx.error??requestError??aborted());};
   tx.onerror=event=>{requestError=event.target?.error??tx.error;};
   signal?.addEventListener('abort',cancel,{once:true});
   const track=request=>{request.onerror=()=>{requestError=request.error;};return request;};
   try{result=work(Object.fromEntries(names.map(name=>[name,tx.objectStore(name)])),track)??result;}catch(error){requestError=error;cancel();}
  });
 }
 return {
  // The save JSON bytes, or null. A corrupt record fails the gzip CRC.
  async read(name){
   const record=await transaction([SAVES],'readonly',(stores,track)=>{const request=track(stores.saves.get(name));return ()=>request.result??null;});
   if(record===null)return null;
   if(!(record instanceof Uint8Array))throw new Error('Invalid save record.');
   return gunzip(record);
  },
  list(){return transaction([INDEX,META],'readonly',(stores,track)=>{
   const entries=track(stores.index.getAll()),last=track(stores.meta.get(LAST));
   return ()=>{
    const all=entries.result??[];
    return {saves:all.filter(entry=>entry.name!==AUTOSAVE).sort(byName),autosave:all.find(entry=>entry.name===AUTOSAVE)??null,last:last.result??null};
   };
  });},
  // `save`: the Rust save JSON bytes.
  async write(name,save,entry,{signal}={}){
   checkSignal(signal);
   if(!(save instanceof Uint8Array)||!save.length)throw new Error('Invalid save payload.');
   const record=await gzip(save);
   await transaction(STORES,'readwrite',(stores,track)=>{
    track(stores.saves.put(record,name));track(stores.index.put(entry,name));track(stores.meta.put(name,LAST));
   },signal);
  },
  remove(name){return transaction(STORES,'readwrite',(stores,track)=>{
   track(stores.saves.delete(name));track(stores.index.delete(name));
   const last=track(stores.meta.get(LAST));
   last.onsuccess=()=>{if(last.result===name)track(stores.meta.delete(LAST));};
  });},
  close(){
   closed=true;opening?.fail(new Error('Save storage is closed.'));
   for(const tx of transactions){try{tx.abort();}catch{}}
   connection?.close();connection=null;
  }
 };
}
