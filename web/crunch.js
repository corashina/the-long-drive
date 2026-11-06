// Crunch textures unpack to BC blocks in a pool of workers, so the main thread only copies.
// The pool starts on the first job. Each job goes to the worker with the fewest jobs.
export function installCrunch(target,size=Math.max(1,Math.min(4,(navigator.hardwareConcurrency||2)-2))){
 const workers=[],jobs=new Map();let next=0;
 const start=()=>{
  for(let i=0;i<size;i++){
   const worker=new Worker(new URL('./crunch-worker.js',import.meta.url),{type:'module'});
   const entry={worker,pending:0};
   worker.addEventListener('message',({data})=>{
    const job=jobs.get(data.id);jobs.delete(data.id);entry.pending--;
    if(data.error!==undefined)job.reject(Error(data.error));else job.resolve(data.blocks);
   });
   worker.addEventListener('error',event=>{
    // A worker that fails to start fails its jobs. The game reports the texture load failure.
    event.preventDefault();
    for(const [id,job] of jobs)if(job.entry===entry){jobs.delete(id);job.reject(Error('Crunch worker: '+(event.message||'failed')));}
    entry.pending=0;
   });
   workers.push(entry);
  }
 };
 // `file` can be a view of WASM memory. The copy owns its buffer, so it transfers.
 target.crunchUnpack=(file,first)=>{
  if(!workers.length)start();
  const copy=file.slice(),id=next++;
  const entry=workers.reduce((a,b)=>b.pending<a.pending?b:a);
  entry.pending++;
  return new Promise((resolve,reject)=>{
   jobs.set(id,{resolve,reject,entry});
   entry.worker.postMessage({id,file:copy,first},[copy.buffer]);
  });
 };
}
