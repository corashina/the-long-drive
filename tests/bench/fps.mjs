// Steady-state F3 bench. It starts a new game with a fixed seed, moves the player with the
// `?sandbox` page hooks and records the F3 values of each scene as JSON.
//
//   node tests/bench/fps.mjs [--label master] [--url http://localhost:4173/] [--seed 1122282040]
//     [--scenes garage,road,town] [--town NAME] [--settle 10] [--seconds 20] [--trace] [--headless]
//     [--channel chrome]   (the installed Google Chrome; an empty value takes the Playwright Chromium)
//   node tests/bench/fps.mjs compare A.json B.json
//
// Run headed on the real GPU. Headless Chrome can take a software adapter: then GPU ms are null.
// Serve the build on port 4173 first. Nothing else heavy may run on the PC during a run.
// Output: tests/bench/out/<label>-<time>.json (and a Chrome trace of the last scene with --trace).
import {chromium} from 'playwright-core';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';

const OUT=fileURLToPath(new URL('./out/',import.meta.url));
const START_TIMEOUT_MS=600000,MOVE_TIMEOUT_MS=180000;
// Values that the summary and the compare table show, in this order.
const FIELDS=['fps','frameMs','worstMs','updateMs','renderMs','workerTickMs','gpuShadow','gpuOpaque','gpuResolveAo','gpuTransparent','gpuPost','gpuSum','shadowDraws','surfaceDraws','propVisible','sceneScale'];
const GPU=['gpuShadow','gpuOpaque','gpuResolveAo','gpuTransparent','gpuPost'];

// Scene actions for `simulation::sandbox`. `noon` and `night` are source cycle seconds.
const SCENES={
 garage:({noon})=>[{action:'garage'},{action:'cycle',seconds:noon}],
 road:({noon})=>[{action:'road',distance:2000},{action:'cycle',seconds:noon}],
 town:({night,town})=>[{action:'building',name:town},{action:'cycle',seconds:night}],
};

function options(argv){
 const o={label:'build',url:'http://localhost:4173/',seed:'1122282040',scenes:'garage,road,town',town:'',settle:10,seconds:20,trace:false,headless:false,channel:'chrome'};
 for(let i=0;i<argv.length;i++){
  const name=argv[i].replace(/^--/,'');
  if(!(name in o))throw new Error('Unknown option '+argv[i]);
  if(typeof o[name]==='boolean')o[name]=true;
  else{const value=argv[++i];o[name]=typeof o[name]==='number'?Number(value):value;}
 }
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
 const failure=await page.evaluate(()=>window.gameFailure?.()??null);
 if(failure)throw new Error('Game failed: '+failure);
}

async function worldState(page){
 const [reply]=await act(page,[{action:'state',radius:1}]);
 return reply.data;
}

function flatten(snapshot){
 const row={...snapshot};
 delete row.gpuMs;
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

function summarize(samples){
 const summary={};
 for(const field of FIELDS){
  const values=samples.map(s=>s[field]);
  summary[field]={median:percentile(values,0.5),p90:percentile(values,0.9)};
 }
 return summary;
}

// `log` collects console errors, warnings, page errors and page loads for the JSON.
async function startGame(browser,o,log){
 const context=await browser.newContext({viewport:{width:1920,height:1080}});
 // A new context has empty storage, so every setting is on its default.
 await context.addInitScript(seed=>{
  if(sessionStorage.getItem('bench-started'))return;
  sessionStorage.setItem('bench-started','1');
  sessionStorage.setItem('tls-new-game',seed);
 },o.seed);
 const page=await context.newPage();
 page.on('console',m=>{if(m.type()==='error'||m.type()==='warning')log.errors.push(m.text());});
 page.on('pageerror',e=>log.errors.push(e.message));
 page.on('framenavigated',frame=>{if(frame===page.mainFrame())log.loads++;});
 const url=new URL(o.url);url.searchParams.set('sandbox','');
 await page.goto(url.href);
 // The seed makes the start menu begin a new game. If the start menu stays open, select New game.
 let last=null,clicked=0;
 try{
  await until('game start',START_TIMEOUT_MS,async()=>{
   // A new game can reload the page. Then the evaluate fails, and the next poll reads the new page.
   const state=await page.evaluate(()=>({failure:window.gameFailure?.()??null,startupDone:Boolean(window.gameStartupDone?.()),
    started:Boolean(window.gameState?.started),paused:Boolean(window.gameState?.paused),
    newGame:Boolean(document.querySelector('#new-game')?.checkVisibility()),status:document.querySelector('#status')?.textContent??''})).catch(error=>{
    if(/context was destroyed|navigation/i.test(error.message))return null;
    throw error;
   });
   if(!state)return false;
   last=state;
   if(last.failure)throw new Error('Game failed: '+last.failure);
   if(last.startupDone&&last.started)return true;
   if(last.startupDone&&last.newGame&&Date.now()-clicked>15000){
    clicked=Date.now();
    await page.locator('#new-game').click().catch(()=>{});
   }
   return false;
  });
 }catch(error){
  error.message+=` (last state ${JSON.stringify(last)}, page loads ${log.loads})`;
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
 await sleep(o.settle*1000);
 await checkGame(page);
 const trace=o.trace&&name===o.scenes.split(',').at(-1);
 const tracePath=trace?OUT+o.label+'-'+name+'-trace.json':null;
 if(trace)await browser.startTracing(page,{path:tracePath});
 const samples=[];
 for(let i=0;i<o.seconds;i++){
  await sleep(1000);
  await checkGame(page);
  samples.push(flatten(await page.evaluate(()=>window.gamePerformance())));
 }
 if(trace)await browser.stopTracing();
 // No frame in the 2 s window: the game stopped drawing, so the GPU values are stale.
 if(!samples.some(s=>Number.isFinite(s.fps)))throw new Error('No game frames in '+name);
 const state=await worldState(page);
 return {name,moved:replies[0].message,player:state.player,cycle:state.cycle,summary:summarize(samples),samples,trace:tracePath};
}

async function bench(o){
 mkdirSync(OUT,{recursive:true});
 const browser=await chromium.launch({headless:o.headless,channel:o.channel||undefined,args:['--enable-unsafe-webgpu','--enable-webgpu-developer-features']});
 const log={errors:[],loads:0},scenes=[];
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
 const result={label:o.label,seed:o.seed,url:o.url,date:new Date().toISOString(),adapter,scenes,errors:log.errors,pageLoads:log.loads,error:failure?.message??null};
 const path=OUT+o.label+'-'+result.date.replace(/[:.]/g,'-')+'.json';
 writeFileSync(path,JSON.stringify(result,null,1));
 console.log('Wrote '+path);
 if(failure)throw failure;
}

const show=v=>v===null||v===undefined?'-':Number.isInteger(v)?String(v):v.toFixed(2);

function print(label,scene){
 console.log(`\n${label} / ${scene.name}: ${scene.moved}`);
 for(const field of FIELDS){
  const {median,p90}=scene.summary[field];
  console.log(`  ${field.padEnd(15)} median ${show(median).padStart(9)}   p90 ${show(p90).padStart(9)}`);
 }
}

// Median of each field in A and B, with the change in percent.
function compare(pathA,pathB){
 const [a,b]=[pathA,pathB].map(p=>JSON.parse(readFileSync(p,'utf8')));
 for(const scene of a.scenes){
  const other=b.scenes.find(s=>s.name===scene.name);
  if(!other)continue;
  console.log(`\n${scene.name}: ${a.label} -> ${b.label}`);
  for(const field of FIELDS){
   const x=scene.summary[field].median,y=other.summary[field].median;
   const change=Number.isFinite(x)&&Number.isFinite(y)&&x!==0?((y-x)/x*100).toFixed(1)+' %':'-';
   console.log(`  ${field.padEnd(15)} ${show(x).padStart(9)} ${show(y).padStart(9)} ${change.padStart(9)}`);
  }
 }
}

const argv=process.argv.slice(2);
if(argv[0]==='compare')compare(argv[1],argv[2]);
else await bench(options(argv));
