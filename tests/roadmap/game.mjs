// One game tab for a roadmap run. It drives the `?sandbox` page through `tests/sandbox/helpers.js`.
import {readFile} from 'node:fs/promises';
import {join} from 'node:path';

const HELPERS=new URL('../sandbox/helpers.js',import.meta.url);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));

export class Game{
 constructor(page,url,outDir){this.page=page;this.url=url;this.outDir=outDir;this.errors=[];this.shots=[];
  page.on('pageerror',e=>this.errors.push('pageerror: '+e.message));
  page.on('console',m=>{if(m.type()==='error')this.errors.push(m.text());});
 }
 // Loads the page, pastes the helpers and records the survival values in `__needs`.
 async open(query=''){
  this.errors.length=0;
  await this.page.goto(this.url+(query?'&'+query:''));
  await this.page.waitForFunction(()=>!document.querySelector('#new-game').disabled||window.gameState?.started||!document.querySelector('#runtime-error').hidden,null,{timeout:600_000,polling:500});
  await this.failure();
  await this.page.addScriptTag({content:await readFile(HELPERS,'utf8')});
  await this.page.evaluate(()=>{const take=window.gameSurvival;window.__needs={};
   window.gameSurvival=(hp,food,water,bowel,bladder,opacity,dead,...rest)=>{Object.assign(window.__needs,{hp,food,water,dead});take(hp,food,water,bowel,bladder,opacity,dead,...rest);};});
 }
 // Starts a New game and waits until the world holds the player.
 async newGame(){await this.page.click('#new-game');await this.playing();}
 // Waits until the game runs. The helpers press Continue on the pause menu.
 async playing(seconds=600){
  await this.page.evaluate(()=>{window.__auto=true;});
  await this.waitFor(()=>document.querySelector('#menu').hidden&&window.gameState.started,seconds,'game start');
  await this.world();
 }
 // Waits until the stream holds the player.
 async world(seconds=120){
  const end=Date.now()+seconds*1000;
  for(;;){const s=await this.state(10);if(s.supported&&s.required_ready)return s;if(Date.now()>end)throw Error('World not ready: player_gap '+JSON.stringify(s.player_gap));await sleep(500);}
 }
 // Throws with the game failure text, if any.
 async failure(){
  const text=await this.page.evaluate(()=>{const e=document.querySelector('#runtime-error');const s=document.querySelector('#status').textContent;
   return !e.hidden?e.textContent:/Cannot start/.test(s)?s:'';});
  if(text)throw Error(text);
 }
 async waitFor(fn,seconds,what,arg){
  try{await this.page.waitForFunction(fn,arg,{timeout:seconds*1000,polling:250});}
  catch{await this.failure();throw Error('Timed out after '+seconds+' s: '+what);}
 }
 // Calls `window.sandbox[action]`. A reply with `ok: false` throws.
 async sandbox(action,...args){
  const reply=await this.page.evaluate(([a,args])=>window.sandbox[a](...args),[action,args]);
  if(!reply.ok)throw Error(action+': '+reply.message);
  return reply;
 }
 async state(radius=50){return (await this.page.evaluate(r=>window.sandbox.state(r),radius)).data;}
 eval(fn,arg){return this.page.evaluate(fn,arg);}
 game(){return this.page.evaluate(()=>({...window.gameState,needs:window.__needs,f3:window.__f3()}));}
 tap(code,ms){return this.page.evaluate(([c,ms])=>window.__keys.tap(c,ms),[code,ms]);}
 click(button=0,ms){return this.page.evaluate(([b,ms])=>window.__keys.mouse(b,ms),[button,ms]);}
 sleep(seconds){return sleep(seconds*1000);}
 // F3 FPS samples, one per second. Shows the F3 panel first.
 async fps(seconds=10){
  await this.page.evaluate(()=>{const p=document.querySelector('#performance');if(p.hidden)window.dispatchEvent(new KeyboardEvent('keydown',{code:'F3'}));});
  const out=[];for(let i=0;i<seconds;i++){await sleep(1000);const m=/FPS: (\d+)/.exec(await this.page.textContent('#performance-values'));if(m)out.push(Number(m[1]));}
  return out;
 }
 // Escape to the pause menu. The helpers stop pressing Continue until `playing()`.
 async pause(){
  await this.page.evaluate(()=>{window.__auto=false;});
  await this.page.keyboard.press('Escape');
  await this.waitFor(()=>!document.querySelector('#pause-actions').hidden,10,'pause menu');
 }
 // Saves a canvas screenshot for the tester. Answers the path.
 async shot(name){const path=join(this.outDir,name.replace(/[^\w.-]+/g,'-')+'.png');await this.page.screenshot({path});this.shots.push(path);return path;}
 // Pause > Save under `name`, then Load last. Waits until the game runs again.
 async saveLoad(name){
  await this.pause();
  await this.page.click('#save');
  await this.page.fill('#save-name',name);
  await this.page.click('#save-save');
  if(await this.page.isVisible('#save-save-yes'))await this.page.click('#save-save-yes');
  await this.waitFor(n=>[...document.querySelectorAll('.save-tile strong')].some(s=>s.textContent===n),60,'save tile',name);
  await this.page.click('#save-last');
  await this.page.click('#save-last-yes');
  await this.playing();
 }
}

// Row results. LOOK: the checks passed, a screenshot needs the tester's eyes.
export const pass=evidence=>({status:'PASS',evidence});
export const fail=evidence=>({status:'FAIL',evidence});
export const look=evidence=>({status:'LOOK',evidence});
export const skip=evidence=>({status:'SKIP',evidence});
export const check=(ok,evidence)=>ok?pass(evidence):fail(evidence);
export const distance=(a,b)=>Math.hypot(a[0]-b[0],a[2]-b[2]);
