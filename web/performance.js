const WINDOW_MS=2000,CAPACITY=4096;

export class FrameTimings {
 constructor(){
  this.times=new Float64Array(CAPACITY);this.intervals=new Float64Array(CAPACITY);
  this.physics=new Float64Array(CAPACITY);this.world=new Float64Array(CAPACITY);this.reset();
 }
 reset(){this.head=0;this.count=0;this.previous=null;}
 sample(now,physics,world){
  const previous=this.previous;this.previous=now;
  if(previous===null||now<=previous)return;
  const i=this.head;
  this.times[i]=now;this.intervals[i]=now-previous;this.physics[i]=physics;this.world[i]=world;
  this.head=(i+1)%CAPACITY;this.count=Math.min(this.count+1,CAPACITY);
  while(this.count&&this.times[(this.head-this.count+CAPACITY)%CAPACITY]<now-WINDOW_MS)this.count--;
 }
 summary(){
  if(!this.count)return null;
  let elapsed=0,worst=0,physics=0,world=0;
  for(let n=0;n<this.count;n++){
   const i=(this.head-this.count+n+CAPACITY)%CAPACITY;
   elapsed+=this.intervals[i];worst=Math.max(worst,this.intervals[i]);physics+=this.physics[i];world+=this.world[i];
  }
  return {fps:1000*this.count/elapsed,average:elapsed/this.count,worst,physics:physics/this.count,world:world/this.count};
 }
}

// GPU ms per world view stage, from timestamp queries. Unavailable without the browser feature.
export function gpuLine(stages){
 if(!stages)return '\nGPU ms: unavailable';
 const total=stages.reduce((a,b)=>a+b,0);
 return '\nGPU ms shadow / opaque / resolve+AO / transparent / post: '+stages.map(v=>v.toFixed(2)).join(' / ')+' (sum '+total.toFixed(2)+')';
}

// Draws per pass in the world view. Counts come from the renderer audit.
function renderLines(audit){
 if(!audit?.graph)return '';
 const view=audit.graph.views[0],sum=values=>values.reduce((a,b)=>a+b,0);
 const [rows,visible,draws]=view.props;
 return '\nDraws shadow / surface: '+sum(view.shadow_issued)+' / '+(view.surface[2]+view.surface[4])+
  '\nGPU props rows / visible / indirect draws: '+rows+' / '+visible+' / '+draws+gpuLine(audit.gpu_ms);
}

// `readWorker` gives the simulation worker tick average since its last read and its WASM bytes.
export function createPerformance(readWasmBytes,readRender,readDrawDistance,readWorker=()=>null){
 const root=document.querySelector('#performance'),values=document.querySelector('#performance-values');
 const timings=new FrameTimings();let lastPaint=-Infinity,stopped=false;
 const ms=value=>value.toFixed(1),mib=bytes=>Number.isFinite(bytes)?(bytes/1048576).toFixed(1)+' MiB':'unavailable';
 function reset(){timings.reset();lastPaint=-Infinity;values.textContent='Waiting for game frames...';}
 const key=event=>{
  if(event.code!=='F3'||event.repeat||event.ctrlKey||event.altKey||event.metaKey)return;
  event.preventDefault();root.hidden=!root.hidden;lastPaint=-Infinity;
 };
 window.addEventListener('keydown',key);document.addEventListener('visibilitychange',reset);
 return {
  frame(now,physicsMs,worldMs){
   if(stopped||document.hidden)return false;
   timings.sample(now,physicsMs,worldMs);
   if(root.hidden||now-lastPaint<250)return false;
   lastPaint=now;return true;
  },
  counters(steps,wheelContacts,bodies,colliders,roads,terrain,objects,jobs,pending,cpuBytes,gpuBytes,storms,stormInfluence,stormParticles,entities){
   const sample=timings.summary(),worker=readWorker();
   values.textContent=(sample?
    'FPS: '+sample.fps.toFixed(0)+'\nFrame avg / worst: '+ms(sample.average)+' / '+ms(sample.worst)+' ms'+
    '\nWorker physics / world CPU avg: '+ms(sample.physics)+' / '+ms(sample.world)+' ms':
    'FPS: --\nFrame avg / worst: --\nWorker physics / world CPU avg: --')+
    '\nWorker tick avg: '+(Number.isFinite(worker?.tickMs)?ms(worker.tickMs)+' ms':'--')+
    '\nDraw distance: '+readDrawDistance()+' m\nPhysics steps/frame: '+steps+'\nWheel contacts: '+wheelContacts+'\nBodies / colliders: '+bodies+' / '+colliders+
    '\nRoads / terrain tiles: '+roads+' / '+terrain+'\nActive generated objects: '+objects+'\nECS entities: '+entities+'\nJobs / queued work: '+jobs+' / '+pending+
    '\nStorms / strongest influence / particles: '+storms+' / '+stormInfluence.toFixed(2)+' / '+stormParticles+
    '\nWASM main / worker: '+mib(readWasmBytes())+' / '+mib(worker?.wasmBytes)+'\nJS heap: '+mib(performance.memory?.usedJSHeapSize)+
    '\nWorld render CPU reserve: '+mib(cpuBytes)+'\nWorld render GPU reserve: '+mib(gpuBytes)+renderLines(readRender());
  },
  stop(){stopped=true;timings.reset();values.textContent='Performance counters stopped.';document.removeEventListener('visibilitychange',reset);},
 };
}
