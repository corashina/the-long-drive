import {playerRows} from './room-card.js';

export function formatBudget(used,limit,unit){
 return unit==='bytes'
  ? (used/1048576).toFixed(1)+' / '+(limit/1048576).toFixed(1)+' MiB'
  : used.toLocaleString('en-US')+' / '+limit.toLocaleString('en-US');
}

// Weights follow the startup trace. Steps after `host` run at the same time. `join` steps show only while the boot joins a room.
const STEPS=[
 {id:'download',label:'Download game data',weight:3,boot:true},
 {id:'code',label:'Start game code',weight:1,boot:true},
 {id:'interface',label:'Load interface',weight:1,boot:true},
 {id:'connect',label:'Connect to the host',weight:2,boot:true,join:true},
 {id:'host',label:'Download the host world',weight:8,boot:true,join:true},
 {id:'roads',label:'Build roads',weight:15},
 {id:'assets',label:'Load garage assets',weight:35},
 {id:'world',label:'Load world area',weight:35},
 {id:'graphics',label:'Prepare graphics',weight:10},
];
const MARK={waiting:'○',active:'●',done:'✓',failed:'✕'};

// `join(code)` shows the room, its players and the join steps. The boot choice calls it.
export function createLoading(){
 const root=document.querySelector('#loading-details'),heading=document.querySelector('#loading-phase');
 const bar=document.querySelector('#loading-progress'),list=document.querySelector('#loading-steps');
 const elapsed=document.querySelector('#loading-elapsed'),resources=document.querySelector('#loading-resources');
 const budgets=document.querySelector('#loading-budgets'),rows=new Map(),budgetValues=new Map();
 const roomNode=document.querySelector('#loading-room');
 const roomPlayers=document.querySelector('#loading-room-players'),roomCode=document.querySelector('#loading-room-code');
 const steps=STEPS.map(step=>{
  const node=document.createElement('li'),mark=document.createElement('span'),label=document.createElement('span'),count=document.createElement('span');
  label.textContent=step.label;node.append(mark,label,count);list.append(node);
  return {...step,node,mark,count,done:0,total:0,note:'',shown:''};
 });
 const byId=new Map(steps.map(step=>[step.id,step]));
 let active=false,timer,started,lastChange,lastPaint=0,percent=0,worldOnly=false,joining=false;
 const seconds=ms=>Math.floor(ms/60000)+'m '+Math.floor(ms/1000)%60+'s';
 const state=step=>step.failed?'failed':step.total>0&&step.done>=step.total?'done':step.total>0||step.note?'active':'waiting';
 const visible=step=>step.join?joining:!(worldOnly&&step.boot);
 function clock(){const now=performance.now();elapsed.textContent='Elapsed: '+seconds(now-started)+'\nLast change: '+seconds(now-lastChange)+' ago';}
 function measure(){
  let sum=0,weights=0;
  for(const step of steps)if(visible(step)){weights+=step.weight;if(step.total>0)sum+=step.weight*Math.min(step.done/step.total,1);}
  // The shown percent does not go down in one load. The step counts show the true values.
  percent=Math.max(percent,Math.floor(100*sum/weights));
 }
 function paint(){
  heading.textContent='Loading '+percent+'%';bar.value=percent;
  for(const step of steps){
   const now=state(step),text=step.note||(step.total>1?step.done.toLocaleString('en-US')+' / '+step.total.toLocaleString('en-US'):'');
   const shown=now+text;
   if(step.shown===shown)continue;
   step.shown=shown;step.node.dataset.state=now;step.mark.textContent=MARK[now];step.count.textContent=text;
  }
  clock();lastPaint=performance.now();
 }
 function set(id,done,total,note='',failed=false){
  const step=byId.get(id);
  done=Math.min(done,total);
  if(step.done===done&&step.total===total&&step.note===note&&step.failed===failed)return false;
  step.done=done;step.total=total;step.note=note;step.failed=failed;lastChange=performance.now();return true;
 }
 function update(changed,force=false){
  if(!changed&&!force)return;
  measure();
  if(force||performance.now()-lastPaint>=250)paint();
 }
 function start(world=false){
  if(active)return;
  // Only the first load of a page joins. New game and Load reload the page.
  if(world)joining=false;roomNode.hidden=!joining;
  active=true;worldOnly=world;percent=0;started=lastChange=performance.now();root.hidden=false;resources.hidden=true;
  budgetValues.clear();
  for(const node of rows.values())node.textContent='';
  for(const step of steps){
   step.node.hidden=!visible(step);step.shown='';
   if(!step.boot||!world){step.done=0;step.total=0;step.note='';step.failed=false;}
  }
  update(true,true);timer=setInterval(paint,1000);
 }
 start();
 return {
  start,
  /** One boot step. `note` replaces the count, for example a download size. */
  step(id,done,total=1,note=''){
   if(!active)return;
   const step=byId.get(id);
   update(set(id,done,total,note),state(step)==='done');
  },
  /** Shows the join steps for room `code`, or hides them for ''. Each call restarts the join steps. */
  join(code){
   if(!active)return;
   joining=Boolean(code);roomCode.textContent=code;roomNode.hidden=!joining;
   roomPlayers.replaceChildren();
   for(const step of steps){step.node.hidden=!visible(step);step.shown='';if(step.join){step.done=0;step.total=0;step.note='';step.failed=false;}}
   update(true,true);
  },
  /** The room players of the join, `[{name,host,you}]`. */
  players(list){
   if(!active||!joining)return;
   roomPlayers.replaceChildren(...playerRows(document,list));
  },
  progress(roads,roadTotal,assets,assetTotal,world,worldTotal,gates,gateTotal){
   if(!active)return;
   // The index goes on past the start roads. The count shows only the start roads.
   const changed=[set('roads',roads,roadTotal),set('assets',assets,assetTotal),set('world',world,worldTotal),set('graphics',gates,gateTotal)];
   update(changed.some(Boolean));
  },
  budget(name,used,limit,unit){
   if(!active)return;
   let node=rows.get(name);
   if(!node){const label=document.createElement('dt');label.textContent=name;node=document.createElement('dd');budgets.append(label,node);rows.set(name,node);}
   const value=budgetValues.get(name);
   if(value){value.used=used;value.limit=limit;value.unit=unit;}else budgetValues.set(name,{name,used,limit,unit});
   const text=formatBudget(used,limit,unit);
   if(node.textContent!==text){node.textContent=text;lastChange=performance.now();}
   node.title=used.toLocaleString('en-US')+' '+unit+' / '+limit.toLocaleString('en-US')+' '+unit;
   resources.hidden=false;
  },
  finish(){active=false;clearInterval(timer);root.hidden=true;},
  fail(){if(!active)return;measure();paint();active=false;clearInterval(timer);heading.textContent='Loading stopped at '+percent+'%';},
  snapshot(){return {percent,steps:steps.filter(visible).map(({id,done,total,note})=>({id,done,total,note})),budgets:Array.from(budgetValues.values(),value=>({...value}))};},
 };
}
