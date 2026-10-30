// Browser test helpers for `?sandbox` runs. Paste into the game tab after the page loads, before New game.
// Mutes audio into an analyser, stubs pointer lock, blocks blur, resumes the pause menu, and adds key and mouse helpers.
// Then `__step(name, fn)` records each test step with the sandbox state into `__run`.
(()=>{const orig=AudioNode.prototype.connect;window.__audio=null;
AudioNode.prototype.connect=function(dst,...a){if(dst instanceof AudioDestinationNode){const ctx=dst.context;if(!ctx.__mute){const an=ctx.createAnalyser();an.fftSize=2048;const g=ctx.createGain();g.gain.value=0;orig.call(an,g);orig.call(g,dst);ctx.__mute={an,g};window.__audio=ctx.__mute;window.__ctx=ctx;}return orig.call(this,ctx.__mute.an,...a);}return orig.call(this,dst,...a);};
window.__level=async(ms=1000)=>{const m=window.__audio;if(!m)return 'no audio';const b=new Float32Array(2048);let peak=0,sum=0,n=0;const t0=performance.now();while(performance.now()-t0<ms){m.an.getFloatTimeDomainData(b);for(const v of b){peak=Math.max(peak,Math.abs(v));sum+=v*v;n++;}await new Promise(r=>setTimeout(r,50));}return {peak:+peak.toFixed(4),rms:+Math.sqrt(sum/n).toFixed(5),state:window.__ctx.state};};
let __lock=null;Object.defineProperty(document,'pointerLockElement',{configurable:true,get:()=>__lock});
HTMLCanvasElement.prototype.requestPointerLock=function(){__lock=this;document.dispatchEvent(new Event('pointerlockchange'));return Promise.resolve();};
document.exitPointerLock=function(){__lock=null;document.dispatchEvent(new Event('pointerlockchange'));};
const sl=ms=>new Promise(r=>setTimeout(r,ms));window.__sl=sl;
window.__keys={down:c=>window.dispatchEvent(new KeyboardEvent('keydown',{code:c,key:c,bubbles:true})),up:c=>window.dispatchEvent(new KeyboardEvent('keyup',{code:c,key:c,bubbles:true})),
 tap:async(c,ms=120)=>{__keys.down(c);await sl(ms);__keys.up(c);},
 look:async(dx,dy,steps=10)=>{const cv=document.querySelector('canvas');for(let i=0;i<steps;i++){cv.dispatchEvent(new MouseEvent('mousemove',{movementX:dx/steps,movementY:dy/steps,bubbles:true}));await sl(16);}},
 mdown:b=>document.querySelector('canvas').dispatchEvent(new MouseEvent('mousedown',{button:b,bubbles:true})),mup:b=>window.dispatchEvent(new MouseEvent('mouseup',{button:b,bubbles:true})),
 mouse:async(b,ms=120)=>{__keys.mdown(b);await sl(ms);__keys.mup(b);},
 wheel:d=>document.querySelector('canvas').dispatchEvent(new WheelEvent('wheel',{deltaY:d,bubbles:true,cancelable:true}))};
window.__h=()=>{const hud=document.querySelector('#hud');return [...hud.querySelectorAll('*')].filter(n=>!n.closest('[hidden]')&&n.children.length===0&&n.innerText?.trim()&&!/^[1-4]$/.test(n.innerText.trim())).map(n=>(n.id?n.id+': ':'')+n.innerText.trim().replace(/\n/g,' / ')).join(' | ');};
window.__f3=()=>document.querySelector('#performance-values').innerText.split('\n').slice(0,4).join(' | ');
return 'patched '+(typeof AudioContext);})();
window.__ev=[];window.__pl=[];
if(!window.__blurBlocked){window.addEventListener('blur',e=>e.stopImmediatePropagation(),true);window.__blurBlocked=true;}
window.__auto=true;if(!window.__autoTimer)window.__autoTimer=setInterval(()=>{if(window.__auto&&!document.querySelector('#menu').hidden&&/Continue/.test(document.querySelector('#menu').innerText)&&!document.querySelector('#resume').closest('[hidden]')){__pl.push('autoresume '+Math.round(performance.now()/1000));document.querySelector('#resume').click();}},1000);
window.__car=()=>{const s=window.gameState;return JSON.stringify({kmh:+s.speedKmh.toFixed(1),pos:[s.absoluteX,s.absoluteY,s.absoluteZ].map(Math.round),fuel:+s.fuelLiters.toFixed(2),seated:s.seated,rpm:Math.round(s.rpm),ign:s.ignition,run:s.running,st:s.starter,hb:s.handbrake,gear:s.gear,target:s.target});};
window.__pos=()=>[gameState.absoluteX,gameState.absoluteY,gameState.absoluteZ];
window.__heading=async(ms=400)=>{const a=__pos();__keys.down('KeyW');await __sl(ms);__keys.up('KeyW');await __sl(150);const b=__pos();return Math.atan2(b[0]-a[0],b[2]-a[2])*180/Math.PI;};
window.__goto=async(tx,tz,run=true,maxSteps=60)=>{for(let i=0;i<maxSteps;i++){const p=__pos();const dx=tx-p[0],dz=tz-p[2];const dist=Math.hypot(dx,dz);if(dist<1.2)return 'arrived '+dist.toFixed(1);const h=await __heading(250);const want=Math.atan2(dx,dz)*180/Math.PI;let err=((want-h+540)%360)-180;await __keys.look(-err/0.24,0,6);await __sl(100);if(run)__keys.down('ShiftLeft');__keys.down('KeyW');await __sl(Math.min(1500,dist/(run?4.4:1.5)*1000));__keys.up('KeyW');__keys.up('ShiftLeft');await __sl(100);}const p=__pos();return 'stopped '+p.map(v=>v.toFixed(1)).join(',');};
window.__sweep=async(pitches=[-10,15,35],steps=36)=>{const seen=new Map();let cur=0;for(const p of pitches){await __keys.look(0,(p-cur)/0.24,4);cur=p;await __sl(150);for(let i=0;i<steps;i++){await __keys.look(360/steps/0.24,0,2);await __sl(90);const t=gameState.target;if(t&&!seen.has(t))seen.set(t,p+'/'+Math.round(i*360/steps)+' '+__h().replace(/target-name: [^|]*\|? ?/,''));}}await __keys.look(0,-cur/0.24,4);return [...seen].map(([k,v])=>k+' @'+v);};
window.__aim=async(dx,dy,want)=>{await __keys.look(dx,dy,8);await __sl(250);if(gameState.target&&(!want||want.test(gameState.target)))return gameState.target+' | '+__h();const pat=[[1,0],[0,1],[-1,0],[-1,0],[0,-1],[0,-1],[1,0],[1,0],[1,0],[0,1],[0,1],[0,1],[-1,0],[-1,0],[-1,0],[-1,0]];for(let s=1;s<=2;s++)for(const [a,b] of pat){await __keys.look(a*6*s,b*6*s,1);await __sl(130);if(gameState.target&&(!want||want.test(gameState.target)))return gameState.target+' | '+__h();}return 'none';};
window.__run=[];
window.__step=async(name,fn)=>{let result;try{result=await fn();}catch(e){result='error: '+(e?.message??e);}const state=(await sandbox.state(30)).data;__run.push({name,result,state,f3:__f3()});return JSON.stringify({name,result});};
window.__until=async(fn,seconds=30)=>{const end=performance.now()+seconds*1000;while(performance.now()<end){if(await fn())return true;await __sl(250);}return false;};
// Cycle seconds run through the day, then the night.
window.__night=async()=>{const s=(await sandbox.state()).data;return sandbox.cycle(s.day_seconds+s.night_seconds/2);};
window.__noon=async()=>{const s=(await sandbox.state()).data;return sandbox.cycle(s.day_seconds/2);};
window.__cars=async(radius=60)=>(await sandbox.state(radius)).data.cars.filter(c=>c.near).map(c=>c.name+' '+c.position.map(Math.round).join(','));
'sandbox helpers ok'
