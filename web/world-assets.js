const MAX_WORLD_STAGING=32*1024*1024;
let cpuProfile;
export function setAssetCpuProfile(profile){cpuProfile=profile;}
// Entry bytes of one start pack. Same limit as `PACK_BYTES` in the exporter.
const PACK_BYTES=16*1024*1024;
let contentBase,files=new Map(),plans=new Map(),onFetched=()=>{};
// Start and top pack entries that were not read yet, by file.
let packed=new Map(),packHeld=0;
// Top packs hold the top mips of start textures. They load in order after the game starts.
let topPacks=[],topPackOf=new Map(),topStarted=0,topRead=0;
export const worldBootstrapMeasurements={dependencyWorkers:0,maxDependencyBytes:0,dependencyPayloadCopyBytes:0,inputMetadataBytes:0,dependencyStagingReserve:0};
export const startPackMeasurements={packs:0,topPacks:0,entries:0,read:0,unused:0,heldPeakBytes:0};
function canonical(path){return typeof path==='string'&&!/[^a-zA-Z0-9_./ -]/.test(path)&&!path.split('/').some(p=>!p||p==='.'||p==='..'||p.startsWith(' ')||p.endsWith(' '));}
export function assetUrl(base,path){
 if(typeof base!=='string'||!/^\.?\/?assets\/runtime\/[a-zA-Z0-9_-]+\/$/.test(base)||!canonical(path))throw Error('Invalid package path.');
 return base+path;
}
function checkReceipt(receipt){
 if(!receipt||!canonical(receipt.file)||!Number.isSafeInteger(receipt.bytes)||receipt.bytes<0||receipt.bytes>0xffffffff||!/^[a-f0-9]{64}$/.test(receipt.sha256))throw Error('Invalid asset receipt or size.');
 if(receipt.file.endsWith('.png')&&(!Array.isArray(receipt.image_size)||receipt.image_size.length!==2||receipt.image_size.some(n=>!Number.isInteger(n)||n<=0||n>0xffffffff)||receipt.image_size[0]*receipt.image_size[1]*4>0xffffffff))throw Error('Missing PNG receipt dimensions.');
}
export function configureAssetCatalog(base,catalog,record=()=>{}){
 const nextFiles=new Map(),nextPlans=new Map();let metadata=0;
 for(const receipt of catalog.files){
  checkReceipt(receipt);assetUrl(base,receipt.file);
  if(nextFiles.has(receipt.file))throw Error('Duplicate file receipt.');
  nextFiles.set(receipt.file,receipt);metadata+=384+receipt.file.length*2+receipt.sha256.length*2;
 }
 for(const plan of catalog.plans){
  if(!nextFiles.has(plan.asset_file)||nextFiles.get(plan.input.file)?.sha256!==plan.input.sha256||nextFiles.get(plan.input.file)?.bytes!==plan.input.bytes||nextPlans.has(plan.asset_file))throw Error('Invalid plan receipt.');
  nextPlans.set(plan.asset_file,plan.input);metadata+=384+plan.asset_file.length*2+plan.input.file.length*2;
 }
 if(!Number.isInteger(catalog.dependency_staging_reserve)||catalog.dependency_staging_reserve<=0||catalog.dependency_staging_reserve>=MAX_WORLD_STAGING)throw Error('Invalid dependency staging reservation.');
 if(!Array.isArray(catalog.packs)||!Array.isArray(catalog.top_packs))throw Error('Invalid start pack receipts.');
 const nextPacked=new Set(),nextTopOf=new Map();
 const checkPack=kind=>(pack,index)=>{
  let bytes=0;
  if(pack.file!=='packs/'+kind+'-'+index+'.gz'||!Number.isSafeInteger(pack.bytes)||pack.bytes<0||!/^[a-f0-9]{64}$/.test(pack.sha256)||!Array.isArray(pack.files)||!pack.files.length)throw Error('Invalid start pack receipt.');
  for(const file of pack.files){
   if(!nextFiles.has(file)||nextPacked.has(file))throw Error('Invalid start pack entry.');
   nextPacked.add(file);bytes+=nextFiles.get(file).bytes;
   if(kind==='top')nextTopOf.set(file,index);
  }
  if(bytes>PACK_BYTES)throw Error('Start pack entry bytes exceed the limit.');
 };
 catalog.packs.forEach(checkPack('start'));
 catalog.top_packs.forEach(checkPack('top'));
 contentBase=base;files=nextFiles;plans=nextPlans;onFetched=record;
 worldBootstrapMeasurements.inputMetadataBytes=metadata;
 worldBootstrapMeasurements.dependencyStagingReserve=catalog.dependency_staging_reserve;
 releaseStartPacks();
 topPacks=catalog.top_packs;topPackOf=nextTopOf;topStarted=0;topRead=0;
 Object.assign(startPackMeasurements,{packs:catalog.packs.length,topPacks:topPacks.length,entries:nextPacked.size,read:0,unused:0,heldPeakBytes:0});
 for(const pack of catalog.packs)readPack(base,pack);
}
/** Drops the pack entries that were not read. Later reads fetch single files. */
export function releaseStartPacks(){
 startPackMeasurements.unused+=packed.size;
 packed=new Map();packHeld=0;
}
// The game reads top mips in pack order. A read from top pack `index` starts it and the next one,
// so at most two top packs are held. Entries of earlier top packs that were not read drop.
function readTopPacks(index){
 for(;topStarted<=Math.min(index+1,topPacks.length-1);topStarted++)readPack(contentBase,topPacks[topStarted],topStarted);
 if(index<=topRead)return;
 topRead=index;
 for(const [file,entry] of packed)if(entry.top<index){
  packed.delete(file);startPackMeasurements.unused++;
  if(entry.ready)packHeld-=entry.receipt.bytes;
 }
}
// A pack is gzip at rest. The hosted site sends a Brotli copy of the decoded pack, which fetch decodes.
// Otherwise this stream decodes the gzip. The entry receipts check the decoded bytes.
function readPack(base,pack,top){
 const entries=pack.files.map(file=>{
  const entry={receipt:files.get(file),ready:false,top};
  entry.bytes=new Promise((resolve,reject)=>{entry.resolve=resolve;entry.reject=reject;});
  // A failed entry that nobody reads is not an unhandled rejection.
  entry.bytes.catch(()=>{});
  packed.set(file,entry);
  return entry;
 });
 const owned=packed;
 const url=assetUrl(base,pack.file);let received=0;
 (async()=>{
  const response=await fetch(url);
  if(!response.ok)throw Error('Asset HTTP '+response.status+': '+pack.file);
  const declared=response.headers.get('content-length');
  const encoding=response.headers.get('content-encoding')?.trim().toLowerCase();
  if(declared!==null&&(!/^\d+$/.test(declared)||((!encoding||encoding==='identity')&&Number(declared)!==pack.bytes)))throw Error('Asset declared size differs: '+pack.file);
  if(!response.body)throw Error('Missing asset body.');
  const counted=response.body.pipeThrough(new TransformStream({transform(chunk,control){received+=chunk.byteLength;control.enqueue(chunk);}}));
  const reader=(encoding==='br'?counted:counted.pipeThrough(new DecompressionStream('gzip'))).getReader();
  let index=0,offset=0,bytes=new Uint8Array(entries[0].receipt.bytes);
  const complete=async()=>{
   while(index<entries.length&&offset===bytes.length){
    const entry=entries[index];
    await checkAsset(entry.receipt,bytes);
    entry.ready=true;entry.resolve(bytes);
    if(packed===owned&&packed.get(entry.receipt.file)===entry){
     packHeld+=bytes.length;startPackMeasurements.heldPeakBytes=Math.max(startPackMeasurements.heldPeakBytes,packHeld);
    }
    index++;offset=0;bytes=index<entries.length?new Uint8Array(entries[index].receipt.bytes):null;
   }
  };
  try{
   await complete();
   for(;;){
    const {done,value}=await reader.read();if(done)break;
    for(let at=0;at<value.byteLength;){
     if(index>=entries.length)throw Error('Asset length exceeds receipt: '+pack.file);
     const count=Math.min(bytes.length-offset,value.byteLength-at);
     bytes.set(value.subarray(at,at+count),offset);offset+=count;at+=count;
     await complete();
    }
   }
  }finally{
   try{await reader.cancel();}finally{reader.releaseLock();}
  }
  if(index<entries.length)throw Error('Asset length differs: '+pack.file);
 })().catch(error=>{for(const entry of entries)if(!entry.ready)entry.reject(error);})
  .finally(()=>onFetched({path:url,requiredBytes:pack.bytes,receivedBytes:received}));
}
async function takePacked(receipt,signal){
 const entry=packed.get(receipt.file);
 packed.delete(receipt.file);
 if(entry.ready)packHeld-=receipt.bytes;
 startPackMeasurements.read++;
 if(!signal)return entry.bytes;
 signal.throwIfAborted();
 return new Promise((resolve,reject)=>{
  const abort=()=>reject(signal.reason);
  signal.addEventListener('abort',abort,{once:true});
  entry.bytes.then(resolve,reject).finally(()=>signal.removeEventListener('abort',abort));
 });
}
export function assetReceipt(path){const value=files.get(path);if(!value)throw Error('Missing asset receipt: '+path);return value;}
export function worldInputBytes(path){return assetReceipt(path).bytes;}
export function worldInputMetadataBytes(){return worldBootstrapMeasurements.inputMetadataBytes;}
export function checkImageSize(receipt,width,height){
 if(!receipt.image_size||receipt.image_size[0]!==width||receipt.image_size[1]!==height)throw Error('Invalid decoded image dimensions.');
}
export function readPackageAsset(path,signal){return readAsset(contentBase,assetReceipt(path),signal);}
export async function readAsset(base,receipt,signal){
 checkReceipt(receipt);
 if(base===contentBase&&topPackOf.has(receipt.file))readTopPacks(topPackOf.get(receipt.file));
 // A start pack gives each entry once. A later read of the same file fetches it alone.
 if(base===contentBase&&packed.get(receipt.file)?.receipt.sha256===receipt.sha256)return takePacked(receipt,signal);
 const url=assetUrl(base,receipt.file);let received=0;
 try{
  const response=await fetch(url,{signal});
  let bytes,reader,length=0;
  try{
   if(!response.ok)throw Error('Asset HTTP '+response.status+': '+receipt.file);
   const declared=response.headers.get('content-length');
   const encoding=response.headers.get('content-encoding')?.trim().toLowerCase();
   // Fetch decodes the body, but Content-Length still describes the encoded bytes.
   if(declared!==null&&(!/^\d+$/.test(declared)||((!encoding||encoding==='identity')&&Number(declared)!==receipt.bytes)))throw Error('Asset declared size differs: '+receipt.file);
   if(!response.body)throw Error('Missing asset body.');
   bytes=new Uint8Array(receipt.bytes);reader=response.body.getReader();
   for(;;){const {done,value}=await reader.read();if(done)break;
    received+=value.byteLength;
    if(value.byteLength>receipt.bytes-length)throw Error('Asset length exceeds receipt: '+receipt.file);
    bytes.set(value,length);length+=value.byteLength;
   }
  }finally{
   if(reader){try{await reader.cancel();}finally{reader.releaseLock();}}
   else await response.body?.cancel();
  }
  if(length!==receipt.bytes)throw Error('Asset length differs: '+receipt.file);
  await checkAsset(receipt,bytes);
  return bytes;
 }finally{
  onFetched({path:url,requiredBytes:receipt.bytes,receivedBytes:received});
  worldBootstrapMeasurements.maxDependencyBytes=Math.max(worldBootstrapMeasurements.maxDependencyBytes,received);
  worldBootstrapMeasurements.dependencyPayloadCopyBytes=2*worldBootstrapMeasurements.maxDependencyBytes;
 }
}

async function checkAsset(receipt,bytes){
 const digest=await crypto.subtle.digest('SHA-256',bytes);
 cpuProfile?.begin('assetValidation');
 try {
 const hash=Array.from(new Uint8Array(digest),v=>v.toString(16).padStart(2,'0')).join('');
 if(hash!==receipt.sha256)throw Error('Asset SHA-256 mismatch: '+receipt.file);
 if(receipt.file.endsWith('.png'))checkPngInput(bytes,{width:receipt.image_size[0],height:receipt.image_size[1]});
 } finally {cpuProfile?.end('assetValidation');}
}

// The checked export uses RGB/RGBA8, bounded IDAT chunks, and no auxiliary PNG payloads.
function checkPngInput(bytes,mip){
 const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
 if(bytes.length<45||view.getUint32(0)!==0x89504e47||view.getUint32(4)!==0x0d0a1a0a||view.getUint32(8)!==13||view.getUint32(12)!==0x49484452||view.getUint32(16)!==mip.width||view.getUint32(20)!==mip.height||bytes[24]!==8||![2,6].includes(bytes[25])||bytes[26]||bytes[27]||bytes[28])throw Error('Unsupported checked PNG layout.');
 for(let offset=8;offset<bytes.length;){
  if(offset+12>bytes.length)throw Error('Truncated checked PNG chunk.');
  const size=view.getUint32(offset),type=view.getUint32(offset+4);
  if(![0x49484452,0x49444154,0x49454e44].includes(type)||size>65536||offset+12+size>bytes.length)throw Error('Unsupported checked PNG chunk.');
  offset+=12+size;
 }
}
