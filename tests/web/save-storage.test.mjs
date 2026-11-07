import test from 'node:test';
import assert from 'node:assert/strict';
import {gunzipSync} from 'node:zlib';
import {AUTOSAVE, createSaveStorage, defaultName, trimName} from '../../web/save-storage.js';

const payload=new TextEncoder().encode('{"schema_version":9,"saved_at":"2026-10-12"}');
const tick=()=>new Promise(resolve=>setImmediate(resolve));
const entry=(name)=>({name,date:'',seed:1,drivenKm:0,version:7,thumbnail:null});
// In-memory IndexedDB with several stores. Set hold to delay completion, or failure to abort the next transaction.
function fake({oldVersion=0,stores={}}={}){
 const data=new Map(Object.entries(stores).map(([name,rows])=>[name,new Map(rows)]));
 const f={data,hold:false,failure:null,deleted:[],upgrade:null};
 const db={version:3,objectStoreNames:{contains:name=>data.has(name),[Symbol.iterator]:()=>data.keys()},close(){},createObjectStore(name){data.set(name,new Map());},deleteObjectStore(name){f.deleted.push(name);data.delete(name);},
  transaction(names,mode,options){
   f.args=[names,mode,options].filter(Boolean);const queue=[];
   const tx={aborted:false,abort(){if(tx.aborted)return;tx.aborted=true;tx.onabort?.();},objectStore(name){const rows=data.get(name);const op=run=>{const request={};queue.push(()=>{request.result=run();request.onsuccess?.();});return request;};
    return {get:key=>op(()=>rows.get(key)),getAll:()=>op(()=>[...rows.values()]),put:(value,key)=>op(()=>{rows.set(key,value);}),delete:key=>op(()=>{rows.delete(key);})};}};
   setImmediate(async()=>{
    if(f.failure){tx.error=f.failure;f.failure=null;tx.abort();return;}
    while(queue.length&&!tx.aborted)queue.shift()();
    while(f.hold)await tick();
    if(!tx.aborted)tx.oncomplete();
   });
   return tx;
  }};
 f.indexedDB={open(name,version){f.open=[name,version];const request={transaction:{abort(){request.aborted=true;}}};setImmediate(()=>{request.result=db;if(oldVersion<version)request.onupgradeneeded({oldVersion});if(!request.aborted)request.onsuccess();});return request;}};
 f.storage=createSaveStorage({indexedDB:f.indexedDB});
 return f;
}

test('record_is_the_gzip_of_the_save_bytes_and_corruption_rejects',async()=>{
 const f=fake();await f.storage.write('a',payload,entry('a'));
 const stored=f.data.get('saves').get('a');assert.deepEqual(new Uint8Array(gunzipSync(stored)),payload);
 assert.deepEqual(await f.storage.read('a'),payload);
 const bad=stored.slice();bad[bad.length-8]^=1;f.data.get('saves').set('a',bad);await assert.rejects(f.storage.read('a'));
 f.data.get('saves').set('a',{version:1,payload:{}});await assert.rejects(f.storage.read('a'),/record/i);
});
test('write_fills_all_stores_in_one_strict_transaction_and_waits_for_completion',async()=>{
 const f=fake();f.hold=true;let done=false;const write=f.storage.write('a',payload,entry('a')).then(()=>{done=true;});
 for(let i=0;i<5;i++)await tick();assert.equal(done,false);f.hold=false;await write;
 assert.deepEqual(f.open,['tld-browser-save',3]);assert.deepEqual(f.args,[['saves','index','meta'],'readwrite',{durability:'strict'}]);
 assert.ok(f.data.get('saves').get('a') instanceof Uint8Array);assert.deepEqual(f.data.get('index').get('a'),entry('a'));assert.equal(f.data.get('meta').get('last'),'a');
 assert.equal(await f.storage.read('b'),null);
});
test('upgrade_from_version_2_drops_all_old_stores',async()=>{
 const f=fake({oldVersion:2,stores:{saves:[['manual',{version:1}]],index:[['manual',entry('manual')]],meta:[['last','manual']]}});assert.equal(await f.storage.read('manual'),null);
 assert.deepEqual(f.deleted,['saves','index','meta']);assert.deepEqual([...f.data.keys()].sort(),['index','meta','saves']);
});
test('list_sorts_names_and_puts_autosave_apart',async()=>{
 const f=fake();for(const name of ['b',AUTOSAVE,'a'])await f.storage.write(name,payload,entry(name));
 const list=await f.storage.list();assert.deepEqual(f.args,[['index','meta'],'readonly']);
 assert.deepEqual(list.saves.map(e=>e.name),['a','b']);assert.equal(list.autosave.name,AUTOSAVE);assert.equal(list.last,'a');
});
test('remove_deletes_the_slot_and_clears_last_only_when_equal',async()=>{
 const f=fake();await f.storage.write('a',payload,entry('a'));await f.storage.write('b',payload,entry('b'));
 await f.storage.remove('a');assert.equal((await f.storage.list()).last,'b');assert.equal(f.data.get('saves').has('a'),false);
 await f.storage.remove('b');assert.deepEqual(await f.storage.list(),{saves:[],autosave:null,last:null});assert.equal(f.data.get('saves').size,0);
});
test('unavailable_storage_and_quota_errors_reject',async()=>{
 await assert.rejects(createSaveStorage({indexedDB:undefined}).read('a'),/unavailable/i);
 const f=fake();f.failure=new DOMException('Full','QuotaExceededError');await assert.rejects(f.storage.write('a',payload,entry('a')),{name:'QuotaExceededError'});
});
test('incompatible_upgrade_aborts_without_creating_a_store',async()=>{
 const f=fake({oldVersion:4});await assert.rejects(f.storage.list(),/version/i);assert.equal(f.data.size,0);
});
test('trim_name_drops_control_characters',()=>{
 assert.equal(trimName('a\u0000b\tc\n d́!€'),'abc d!€');
});
test('default_name_uses_local_yy_MM_dd_HHmm',()=>{
 assert.equal(defaultName(new Date(2026,0,5,7,3)),'26_01_05_0703');
});

test('load_checks_the_record_then_reloads_with_the_name',async()=>{
 const {createSaveSession}=await import('../../web/save-session.js');
 const f=fake(); await f.storage.write('a',payload,entry('a')); const reloads=[];
 const session=createSaveSession({drawDistance:()=>2500,reload:async name=>{reloads.push(name);},storage:f.storage,runtime:{}});
 await session.load('a'); assert.deepEqual(reloads,['a']);
});
