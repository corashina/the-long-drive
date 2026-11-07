const WINDOW_MS=2000,CAPACITY=4096;
// `WeatherKind` order.
const WEATHER=['clear','cloudy','rain','thunderstorm','fog'];

export class FrameTimings {
 constructor(){
  this.times=new Float64Array(CAPACITY);this.intervals=new Float64Array(CAPACITY);
  this.physics=new Float64Array(CAPACITY);this.rapier=new Float64Array(CAPACITY);this.world=new Float64Array(CAPACITY);
  this.update=new Float64Array(CAPACITY);this.render=new Float64Array(CAPACITY);this.reset();
 }
 reset(){this.head=0;this.count=0;this.previous=null;}
 sample(now,physics,rapier,world,update,render){
  const previous=this.previous;this.previous=now;
  if(previous===null||now<=previous)return;
  const i=this.head;
  this.times[i]=now;this.intervals[i]=now-previous;this.physics[i]=physics;this.rapier[i]=rapier;this.world[i]=world;this.update[i]=update;this.render[i]=render;
  this.head=(i+1)%CAPACITY;this.count=Math.min(this.count+1,CAPACITY);
  while(this.count&&this.times[(this.head-this.count+CAPACITY)%CAPACITY]<now-WINDOW_MS)this.count--;
 }
 summary(){
  if(!this.count)return null;
  let elapsed=0,worst=0,physics=0,rapier=0,world=0,update=0,render=0;
  for(let n=0;n<this.count;n++){
   const i=(this.head-this.count+n+CAPACITY)%CAPACITY;
   elapsed+=this.intervals[i];worst=Math.max(worst,this.intervals[i]);physics+=this.physics[i];rapier+=this.rapier[i];world+=this.world[i];update+=this.update[i];render+=this.render[i];
  }
  return {fps:1000*this.count/elapsed,average:elapsed/this.count,worst,physics:physics/this.count,rapier:rapier/this.count,world:world/this.count,update:update/this.count,render:render/this.count};
 }
 // Frame intervals of the frames after `time` in the window, oldest first. For the bench only.
 since(time){
  const out=[];
  for(let n=0;n<this.count;n++){
   const i=(this.head-this.count+n+CAPACITY)%CAPACITY;
   if(this.times[i]>time)out.push(this.intervals[i]);
  }
  return out;
 }
}

// GPU ms per world view stage, from timestamp queries. Unavailable without the browser feature.
export function gpuLine(stages){
 if(!stages)return '\nGPU ms: unavailable';
 const total=stages.reduce((a,b)=>a+b,0);
 return '\nGPU ms shadow / opaque / resolve+AO / weather / transparent / post: '+stages.map(v=>v.toFixed(2)).join(' / ')+' (sum '+total.toFixed(2)+')';
}

// Draws per pass in the world view, from the renderer audit. Null before the first graph.
function renderCounts(audit){
 if(!audit?.graph)return null;
 const view=audit.graph.views[0],[rows,visible,draws]=view.props;
 return {shadowDraws:view.shadow_issued.reduce((a,b)=>a+b,0),surfaceDraws:view.surface[2]+view.surface[4],
  propRows:rows,propVisible:visible,propDraws:draws,gpuMs:audit.gpu_ms??null,sceneScale:audit.scene_scale};
}
function renderLines(audit){
 const c=renderCounts(audit);
 if(!c)return '';
 return '\nDraws shadow / surface: '+c.shadowDraws+' / '+c.surfaceDraws+
  '\nGPU props rows / visible / indirect draws: '+c.propRows+' / '+c.propVisible+' / '+c.propDraws+gpuLine(c.gpuMs)+'\nScene scale: '+c.sceneScale+'%';
}

// `readWorker` gives the simulation worker tick average since its last read and its WASM bytes.
// `watchRender` tells the renderer to fill its audit. Frames skip that work while nothing reads it.
// `readWasmBytes` gives the total WASM memory of the page and its workers. `wasmLimit` is its limit.
export function createPerformance(readWasmBytes,wasmLimit,readRender,watchRender,readDrawDistance,readWorker=()=>null){
 const root=document.querySelector('#performance'),values=document.querySelector('#performance-values');
 const timings=new FrameTimings(),budgets=[];let bodyCounts=null,lastPaint=-Infinity,stopped=false,benched=false;
 const ms=value=>value.toFixed(1),mib=bytes=>Number.isFinite(bytes)?(bytes/1048576).toFixed(1)+' MiB':'unavailable';
 function reset(){timings.reset();lastPaint=-Infinity;values.textContent='Waiting for game frames…';}
 const key=event=>{
  if(event.code!=='F3'||event.repeat||event.ctrlKey||event.altKey||event.metaKey)return;
  event.preventDefault();root.hidden=!root.hidden;lastPaint=-Infinity;watchRender(!root.hidden||benched);
 };
 window.addEventListener('keydown',key);document.addEventListener('visibilitychange',reset);
 return {
  // The ms values of one frame, in the order of `game_performance_frame` in src/browser.rs.
  frame(physicsMs,rapierMs,worldMs,updateMs,renderMs){
   if(stopped||document.hidden)return false;
   const now=performance.now();
   timings.sample(now,physicsMs,rapierMs,worldMs,updateMs,renderMs);
   if((root.hidden&&!benched)||now-lastPaint<250)return false;
   lastPaint=now;return true;
  },
  // One world count against its cap. Rust sends the rows before `counters`.
  budget(index,name,used,limit,unit){budgets[index]='\n'+name+': '+used+' / '+limit+' '+unit;},
  counters(steps,wheelContacts,bodies,colliders,storms,stormInfluence,stormParticles,weatherKind,weatherCells,weatherRain,weatherWetness,entities,dynamicBodies,awakeBodies,ccdBodies,contactPairs){
   const sample=timings.summary(),worker=readWorker();
   bodyCounts={dynamicBodies,awakeBodies,ccdBodies,contactPairs};
   values.textContent=(sample?
    'FPS: '+sample.fps.toFixed(0)+'\nFrame avg / worst: '+ms(sample.average)+' / '+ms(sample.worst)+' ms'+
    '\nMain CPU update / render avg: '+ms(sample.update)+' / '+ms(sample.render)+' ms'+
    '\nWorker physics (Rapier) / world CPU avg: '+ms(sample.physics)+' ('+ms(sample.rapier)+') / '+ms(sample.world)+' ms':
    'FPS: --\nFrame avg / worst: --\nMain CPU update / render avg: --\nWorker physics (Rapier) / world CPU avg: --')+
    '\nWorker tick avg: '+(Number.isFinite(worker?.tickMs)?ms(worker.tickMs)+' ms':'--')+
    '\nDraw distance: '+readDrawDistance()+' m\nPhysics steps/frame: '+steps+'\nWheel contacts: '+wheelContacts+'\nBodies / colliders: '+bodies+' / '+colliders+
    '\nBodies dynamic / awake / CCD: '+dynamicBodies+' / '+awakeBodies+' / '+ccdBodies+'\nContact pairs: '+contactPairs+
    budgets.join('')+'\nECS entities: '+entities+
    '\nStorms / strongest influence / particles: '+storms+' / '+stormInfluence.toFixed(2)+' / '+stormParticles+
    '\nWeather '+WEATHER[weatherKind]+' cells '+weatherCells+' rain '+weatherRain.toFixed(2)+' wet '+weatherWetness.toFixed(2)+
    '\nWASM memory: '+mib(readWasmBytes())+' of '+mib(wasmLimit)+'\nJS heap: '+mib(performance.memory?.usedJSHeapSize)+
    renderLines(readRender());
  },
  // The F3 values as numbers, for `tests/bench/fps.mjs`. It reads the worker tick average, as F3 does.
  // The first call starts the renderer audit, so its render counts can be stale.
  // `lastFrame` is the time of the last frame. With `since`, `frameIntervals` has the frame ms of the frames after it.
  snapshot(since){
   if(!benched){benched=true;watchRender(true);}
   const sample=timings.summary(),worker=readWorker();
   return {fps:sample?.fps??null,frameMs:sample?.average??null,worstMs:sample?.worst??null,updateMs:sample?.update??null,renderMs:sample?.render??null,
    physicsMs:sample?.physics??null,rapierMs:sample?.rapier??null,worldMs:sample?.world??null,workerTickMs:worker?.tickMs??null,...bodyCounts,...renderCounts(readRender()),
    lastFrame:timings.previous,frameIntervals:Number.isFinite(since)?timings.since(since):null};
  },
  stop(){stopped=true;timings.reset();values.textContent='Performance counters stopped.';document.removeEventListener('visibilitychange',reset);},
 };
}
