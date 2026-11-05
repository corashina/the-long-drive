// Steady-state F3 bench. It starts a new game with a fixed seed, moves the player with the
// `?sandbox` page hooks and records the F3 values of each scene as JSON.
//
//   node tests/bench/fps.mjs [--label master] [--url http://localhost:4173/] [--seed 1122282040]
//     [--scenes garage,road,town] [--town NAME] [--settle 10] [--seconds 20] [--trace] [--headless]
//     [--channel chrome]   (the installed Google Chrome; an empty value takes the Playwright Chromium)
//     [--quality ultra]    (a Graphics quality preset; empty keeps the setting defaults)
//   node tests/bench/fps.mjs compare A.json B.json
//
// Run headed on the real GPU. Headless Chrome can take a software adapter: then GPU ms are null.
// Chrome runs without vsync and frame rate limit, so FPS can go above the display rate.
// Each scene reports frame ms of all frames in play: mean, p50, p99 and max. Compare frame ms, not FPS.
// Serve the build on port 4173 first. Nothing else heavy may run on the PC during a run.
// Output: tests/bench/out/<label>-<time>.json (and a Chrome trace of the last scene with --trace).
import {chromium} from 'playwright-core';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {PRESETS} from '../../web/quality-presets.js';

const OUT=fileURLToPath(new URL('./out/',import.meta.url));
const START_TIMEOUT_MS=600000,MOVE_TIMEOUT_MS=180000;
// Values that the summary and the compare table show, in this order.
const FIELDS=['fps','frameMs','worstMs','updateMs','renderMs','workerTickMs','physicsMs','rapierMs','worldMs','dynamicBodies','awakeBodies','ccdBodies','contactPairs','gpuShadow','gpuOpaque','gpuResolveAo','gpuTransparent','gpuPost','gpuSum','shadowDraws','surfaceDraws','propVisible','sceneScale'];
const GPU=['gpuShadow','gpuOpaque','gpuResolveAo','gpuTransparent','gpuPost'];

// Scene actions for `simulation::sandbox`. `noon` and `night` are source cycle seconds.
const SCENES={
 garage:({noon})=>[{action:'garage'},{action:'cycle',seconds:noon}],
 road:({noon})=>[{action:'road',distance:2000},{action:'cycle',seconds:noon}],
 town:({night,town})=>[{action:'building',name:town},{action:'cycle',seconds:night}],
};

function options(argv){
 const o={label:'build',url:'http://localhost:4173/',seed:'1122282040',scenes:'garage,road,town',town:'',settle:10,seconds:20,trace:false,headless:false,channel:'chrome',quality:''};
 for(let i=0;i<argv.length;i++){
  const name=argv[i].replace(/^--/,'');
  if(!(name in o))throw new Error('Unknown option '+argv[i]);
  if(typeof o[name]==='boolean')o[name]=true;
  else{const value=argv[++i];o[name]=typeof o[name]==='number'?Number(value):value;}
 }
 if(o.quality&&!Object.hasOwn(PRESETS,o.quality))throw new Error('Unknown quality '+o.quality);
 return o;
}

const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));

async function until(what,timeoutMs,test){
 const end=Date.now()+timeoutMs;
 for(;;){
  const value=await test();
  if(value)return value;
  if(Date.now()>end)throw new Error('Timed out: '+what);
  await sleep(500);
 }
}

// Runs sandbox actions. Every action must answer ok.
async function act(page,actions){
 const replies=await page.evaluate(a=>window.gameSandbox(a),actions);
 for(const reply of replies)if(!reply.ok)throw new Error('Sandbox: '+reply.message);
 return replies;
}

// A startup or runtime failure stops the frames and the world stream. Stop the bench with its message.
async function checkGame(page){
 const failure=await page.evaluate(()=>window.gameFailureMessage?.()??null);
 if(failure)throw new Error('Game failed: '+failure);
}

async function worldState(page){
 const [reply]=await act(page,[{action:'state',radius:1}]);
 return reply.data;
}

function flatten(snapshot){
 const row={...snapshot};
 delete row.gpuMs;delete row.lastFrame;delete row.frameIntervals;
 const gpu=snapshot.gpuMs;
 GPU.forEach((name,i)=>{row[name]=gpu?gpu[i]:null;});
 row.gpuSum=gpu?gpu.reduce((a,b)=>a+b,0):null;
 return row;
}

function percentile(values,p){
 const sorted=values.filter(Number.isFinite).sort((a,b)=>a-b);
 if(!sorted.length)return null;
 return sorted[Math.min(sorted.length-1,Math.floor(p*sorted.length))];
}

// CPU stages with calls, most total time first. `audit` is a Rust `cpu_profile_audit`.
function cpuStages(audit){
 const rows=(names,values)=>names.map((name,i)=>({name,...values[i]}));
 return [...rows(audit.stages,audit.measurements),...rows(audit.streaming.stages,audit.streaming.measurements)]
  .filter(r=>r.calls>0).sort((a,b)=>b.total_ms-a.total_ms)
  .map(({name,calls,total_ms,mean_ms,max_ms,items})=>({name,calls,totalMs:total_ms,meanMs:mean_ms,maxMs:max_ms,items:items?items/calls:null}));
}

// Frame ms of every frame in play. FPS comes from the mean frame ms.
function frameStats(intervals){
 if(!intervals.length)return null;
 const meanMs=intervals.reduce((a,b)=>a+b,0)/intervals.length;
 return {count:intervals.length,fps:1000/meanMs,meanMs,p50Ms:percentile(intervals,0.5),p99Ms:percentile(intervals,0.99),maxMs:Math.max(...intervals)};
}
const FRAME_FIELDS=['fps','meanMs','p50Ms','p99Ms','maxMs'];

function summarize(samples){
 const summary={};
 for(const field of FIELDS){
  const values=samples.map(s=>s[field]);
  summary[field]={median:percentile(values,0.5),p90:percentile(values,0.9)};
 }
 return summary;
}

// `log` collects console errors and warnings, page errors and page events for the JSON.
async function startGame(browser,o,log){
 const context=await browser.newContext({viewport:{width:1920,height:1080}});
 // A new context has empty storage, so every setting is on its default. `--quality` stores a preset first.
 // Each page load logs the reload keys of web/main.js before main.js removes them.
 const settings=o.quality?JSON.stringify({...PRESETS[o.quality],quality:o.quality}):null;
 await context.addInitScript(([seed,settings])=>{
  if(!sessionStorage.getItem('bench-started')){
   sessionStorage.setItem('bench-started','1');
   sessionStorage.setItem('tls-new-game',seed);
   if(settings)localStorage.setItem('the-long-slop.settings',settings);
  }
  const keys=['tls-new-game','tls-load','tls-join'].filter(k=>sessionStorage.getItem(k)!==null).map(k=>k+'='+sessionStorage.getItem(k));
  console.info('[bench] load '+keys.join(' '));
 },[o.seed,settings]);
 const page=await context.newPage();
 const started=Date.now(),event=text=>log.events.push(((Date.now()-started)/1000).toFixed(1)+' s '+text);
 page.on('console',m=>{
  if(m.text().startsWith('[bench]'))event(m.text().slice(8));
  else if(m.type()==='error'||m.type()==='warning')log.errors.push(m.text());
 });
 page.on('pageerror',e=>log.errors.push(e.message));
 page.on('crash',()=>event('page crash'));
 page.on('close',()=>event('page close'));
 const url=new URL(o.url);url.searchParams.set('sandbox','');
 await page.goto(url.href);
 // The seed makes the start menu begin a new game. If the start menu stays idle, select New game.
 let last=null,clicked=0;
 try{
  await until('game start',START_TIMEOUT_MS,async()=>{
   if(page.isClosed())throw new Error('The page closed');
   // A new game can reload the page. Then the evaluate fails, and the next poll reads the new page.
   const state=await page.evaluate(()=>({failure:window.gameFailureMessage?.()??null,startupDone:Boolean(window.gameStartupDone?.()),
    started:Boolean(window.gameState?.started),paused:Boolean(window.gameState?.paused),menu:window.gameMenu?.()??null})).catch(error=>{
    if(/context was destroyed|navigation/i.test(error.message))return null;
    throw error;
   });
   if(!state)return false;
   if(JSON.stringify(state.menu)!==JSON.stringify(last?.menu))event('menu '+JSON.stringify(state.menu));
   last=state;
   if(last.failure)throw new Error('Game failed: '+last.failure);
   if(last.startupDone&&last.started)return true;
   const menu=last.menu;
   if(last.startupDone&&menu?.state==='start-menu'&&menu.ready&&!menu.pending&&!menu.started&&Date.now()-clicked>15000){
    clicked=Date.now();event('click New game');
    await page.locator('#new-game').click().catch(()=>{});
   }
   return false;
  });
 }catch(error){
  error.message+=` (last state ${JSON.stringify(last)})`;
  throw error;
 }
 // The new game can wait in the pause menu. Resume captures the mouse, as a player click does.
 await until('game play',MOVE_TIMEOUT_MS,async()=>{
  await checkGame(page);
  if(!await page.evaluate(()=>window.gameState.paused))return true;
  const resume=page.locator('#resume');
  if(await resume.isVisible())await resume.click();
  return false;
 });
 const adapter=await page.evaluate(async()=>{
  const info=(await navigator.gpu?.requestAdapter())?.info;
  const canvas=document.querySelector('#game');
  return {canvas:[canvas.width,canvas.height],gpu:info?{vendor:info.vendor,architecture:info.architecture,device:info.device,description:info.description}:null};
 });
 return {page,adapter};
}

// Menu state, and Continue when the game left play (focus loss, pointer lock loss, death).
// Out of play, the menu caps frames at 30 fps.
async function resumePlay(page){
 const menu=await page.evaluate(()=>window.gameMenu?.()??null);
 if(menu&&menu.state!=='playing'){
  const resume=page.locator('#resume');
  if(await resume.isVisible())await resume.click().catch(()=>{});
 }
 return menu;
}

async function runScene(browser,page,name,cycle,o){
 const build=SCENES[name];
 if(!build)throw new Error('Unknown scene '+name+'. Scenes: '+Object.keys(SCENES).join(', '));
 const replies=await act(page,build({...cycle,town:o.town}));
 let last;
 try{
  await until(name+' world',MOVE_TIMEOUT_MS,async()=>{
   await checkGame(page);
   last=await worldState(page);
   return last.supported;
  });
 }catch(error){
  // The stream state tells which part of the world never got ready.
  if(last)error.message+=` (pending ${JSON.stringify(last.pending)}, required ready ${last.required_ready}, waiting spawns ${last.world_spawns?.length})`;
  throw error;
 }
 // The first read starts the renderer audit, so the settle time refreshes its counts.
 await page.evaluate(()=>window.gamePerformance());
 await sleep(o.settle*1000);
 await checkGame(page);
 await until(name+' play',MOVE_TIMEOUT_MS,async()=>(await resumePlay(page))?.state==='playing');
 const trace=o.trace&&name===o.scenes.split(',').at(-1);
 const tracePath=trace?OUT+o.label+'-'+name+'-trace.json':null;
 if(trace)await browser.startTracing(page,{path:tracePath});
 // Each read takes the frame ms after the last frame of the previous read. The window holds 2 s of frames.
 const samples=[],intervals=[];
 let since=(await page.evaluate(()=>window.gamePerformance())).lastFrame;
 for(let i=0;i<o.seconds;i++){
  await sleep(1000);
  await checkGame(page);
  const snapshot=await page.evaluate(s=>window.gamePerformance(s),since);
  since=snapshot.lastFrame??since;
  const sample=flatten(snapshot);
  const menu=await resumePlay(page);
  sample.menu=menu?.state??null;
  if(sample.menu!=='playing')sample.menuStatus=menu?.status||menu?.error||'';
  else if(snapshot.frameIntervals)intervals.push(...snapshot.frameIntervals);
  samples.push(sample);
 }
 if(trace)await browser.stopTracing();
 // Only samples in play count. A menu sample has the 30 fps menu cap.
 const playing=samples.filter(s=>s.menu==='playing');
 if(!playing.length)throw new Error(`The game left play in ${name}: ${samples.at(-1).menu} ${samples.at(-1).menuStatus}`);
 // No frame in the 2 s window: the game stopped drawing, so the GPU values are stale.
 if(!playing.some(s=>Number.isFinite(s.fps)))throw new Error('No game frames in '+name);
 // Worker and main thread CPU stages for 5 s. The profile has a cost, so it starts after the samples.
 await page.evaluate(()=>window.gameCpuProfile(true));
 await sleep(5000);
 const [profile]=await act(page,[{action:'profile'}]);
 const main=await page.evaluate(()=>window.gameMainCpuAudit());
 await page.evaluate(()=>window.gameCpuProfile(false));
 const state=await worldState(page);
 return {name,moved:replies[0].message,player:state.player,cycle:state.cycle,frames:frameStats(intervals),summary:summarize(playing),menuSamples:samples.length-playing.length,worker:cpuStages(profile.data),main:cpuStages(main),samples,trace:tracePath};
}

async function bench(o){
 mkdirSync(OUT,{recursive:true});
 const browser=await chromium.launch({headless:o.headless,channel:o.channel||undefined,args:['--enable-unsafe-webgpu','--enable-webgpu-developer-features','--disable-gpu-vsync','--disable-frame-rate-limit']});
 const log={errors:[],events:[]},scenes=[];
 let adapter=null,failure=null;
 // A failure still writes the finished scenes, the error and the console errors.
 try{
  const started=await startGame(browser,o,log);
  adapter=started.adapter;
  const first=await worldState(started.page);
  const cycle={noon:first.day_seconds*0.5,night:first.day_seconds+first.night_seconds*0.5};
  for(const name of o.scenes.split(',')){
   scenes.push(await runScene(browser,started.page,name,cycle,o));
   print(o.label,scenes.at(-1));
  }
 }catch(error){failure=error;}
 finally{await browser.close();}
 const result={label:o.label,seed:o.seed,quality:o.quality||null,url:o.url,date:new Date().toISOString(),adapter,scenes,errors:log.errors,events:log.events,error:failure?.message??null};
 const path=OUT+o.label+'-'+result.date.replace(/[:.]/g,'-')+'.json';
 writeFileSync(path,JSON.stringify(result,null,1));
 console.log('Wrote '+path);
 if(failure){console.log('Page events:\n  '+log.events.join('\n  '));throw failure;}
}

const show=v=>v===null||v===undefined?'-':Number.isInteger(v)?String(v):v.toFixed(2);

function print(label,scene){
 console.log(`\n${label} / ${scene.name}: ${scene.moved}`);
 if(scene.menuSamples)console.log(`  ${scene.menuSamples} samples out of play are left out`);
 const f=scene.frames;
 if(f)console.log(`  frames ${f.count}: mean ${show(f.meanMs)} ms (${show(f.fps)} fps), p50 ${show(f.p50Ms)}, p99 ${show(f.p99Ms)}, max ${show(f.maxMs)} ms`);
 for(const field of FIELDS){
  const {median,p90}=scene.summary[field]??{};
  console.log(`  ${field.padEnd(15)} median ${show(median).padStart(9)}   p90 ${show(p90).padStart(9)}`);
 }
 printStages('worker',scene.worker);
 printStages('main',scene.main);
}

function printStages(thread,stages){
 console.log(`  ${thread} stages (5 s profile): calls, mean ms, max ms, mean items`);
 for(const r of stages.slice(0,16))console.log(`    ${r.name.padEnd(22)} ${String(r.calls).padStart(5)} ${show(r.meanMs).padStart(8)} ${show(r.maxMs).padStart(8)} ${show(r.items).padStart(8)}`);
}

// Median of each field in A and B, with the change in percent.
function compare(pathA,pathB){
 const [a,b]=[pathA,pathB].map(p=>JSON.parse(readFileSync(p,'utf8')));
 for(const scene of a.scenes){
  const other=b.scenes.find(s=>s.name===scene.name);
  if(!other)continue;
  console.log(`\n${scene.name}: ${a.label} -> ${b.label}`);
  const row=(name,x,y)=>{
   const change=Number.isFinite(x)&&Number.isFinite(y)&&x!==0?((y-x)/x*100).toFixed(1)+' %':'-';
   console.log(`  ${name.padEnd(15)} ${show(x).padStart(9)} ${show(y).padStart(9)} ${change.padStart(9)}`);
  };
  for(const field of FRAME_FIELDS)row('frame '+field,scene.frames?.[field],other.frames?.[field]);
  for(const field of FIELDS)row(field,scene.summary[field]?.median,other.summary[field]?.median);
 }
}

const argv=process.argv.slice(2);
if(argv[0]==='compare')compare(argv[1],argv[2]);
else await bench(options(argv));
